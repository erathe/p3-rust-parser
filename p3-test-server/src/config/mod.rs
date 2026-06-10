//! SQLite-backed configuration for the test server.
//!
//! Two tables:
//! - `riders`: the roster bound to TUI keys 1-8
//! - `settings`: key-value store for decoder state and race/timing knobs
//!   (key-value so new settings don't require schema migrations)
//!
//! First open seeds defaults from live-capture values (decoder D0000C00,
//! riders FL-94890 and FL-12345). Future session-recording tables can be
//! added alongside without disturbing these.

use rusqlite::{Connection, params};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("database error")]
    Db(#[from] rusqlite::Error),

    #[error("rider string_id must be exactly 8 ASCII bytes, got {0:?}")]
    InvalidStringId(String),

    #[error("invalid value for setting {key}: {value:?}")]
    InvalidSetting { key: String, value: String },
}

/// A roster entry, bound to TUI key `slot` (1-8)
#[derive(Debug, Clone, PartialEq)]
pub struct Rider {
    pub slot: u8,
    pub transponder_id: u32,
    pub string_id: String,
    pub enabled: bool,
}

impl Rider {
    /// The 8-byte transponder string required by the P3 STRING field
    pub fn string_bytes(&self) -> Result<[u8; 8], ConfigError> {
        self.string_id
            .as_bytes()
            .try_into()
            .map_err(|_| ConfigError::InvalidStringId(self.string_id.clone()))
    }
}

/// All tunable settings, persisted in the `settings` table
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Decoder serial number (0x000C00D0 = "D0000C00" from live capture)
    pub decoder_id: u32,
    /// Background noise level reported in STATUS
    pub noise: u16,
    /// Temperature in tenths of a degree Celsius
    pub temperature_x10: i16,
    /// GPS lock status
    pub gps_fix: bool,
    /// GPS satellites in use
    pub satellites: u8,
    /// STATUS heartbeat interval in seconds
    pub status_interval_s: u64,
    /// Gate beacon transponder ID (9992 = 8m hill, per live capture)
    pub gate_transponder: u32,
    /// Auto-race: winner finish time after gate drop, in seconds
    /// (real BMX is ~30-45s; default compressed for development)
    pub race_winner_finish_s: f64,
    /// Auto-race: minimum gap between consecutive finishers, in seconds
    pub race_gap_min_s: f64,
    /// Auto-race: maximum gap between consecutive finishers, in seconds
    pub race_gap_max_s: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            decoder_id: 0x000C00D0,
            noise: 53,
            temperature_x10: 16,
            gps_fix: true,
            satellites: 0,
            status_interval_s: 5,
            gate_transponder: 9992,
            race_winner_finish_s: 6.0,
            race_gap_min_s: 0.2,
            race_gap_max_s: 1.5,
        }
    }
}

fn default_riders() -> Vec<Rider> {
    vec![
        // From live capture
        Rider {
            slot: 1,
            transponder_id: 102758186,
            string_id: "FL-94890".into(),
            enabled: true,
        },
        Rider {
            slot: 2,
            transponder_id: 123456789,
            string_id: "FL-12345".into(),
            enabled: true,
        },
        // Synthetic fill for fuller heats
        Rider {
            slot: 3,
            transponder_id: 103111222,
            string_id: "FL-31112".into(),
            enabled: true,
        },
        Rider {
            slot: 4,
            transponder_id: 104333444,
            string_id: "FL-43334".into(),
            enabled: true,
        },
    ]
}

pub struct Db {
    conn: Connection,
}

impl Db {
    /// Open (creating and seeding if needed) the config database
    pub fn open(path: &str) -> Result<Self, ConfigError> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    /// In-memory database, for tests
    pub fn open_in_memory() -> Result<Self, ConfigError> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    fn init(&self) -> Result<(), ConfigError> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS riders (
                id             INTEGER PRIMARY KEY,
                slot           INTEGER NOT NULL UNIQUE,
                transponder_id INTEGER NOT NULL UNIQUE,
                string_id      TEXT NOT NULL,
                enabled        INTEGER NOT NULL DEFAULT 1
            );
            CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )?;

        let rider_count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM riders", [], |row| row.get(0))?;
        if rider_count == 0 {
            for rider in default_riders() {
                self.upsert_rider(&rider)?;
            }
        }

        Ok(())
    }

    pub fn load_riders(&self) -> Result<Vec<Rider>, ConfigError> {
        let mut stmt = self.conn.prepare(
            "SELECT slot, transponder_id, string_id, enabled FROM riders ORDER BY slot",
        )?;
        let riders = stmt
            .query_map([], |row| {
                Ok(Rider {
                    slot: row.get(0)?,
                    transponder_id: row.get(1)?,
                    string_id: row.get(2)?,
                    enabled: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(riders)
    }

    pub fn upsert_rider(&self, rider: &Rider) -> Result<(), ConfigError> {
        // Validate eagerly so a bad string never reaches message building
        rider.string_bytes()?;
        self.conn.execute(
            "INSERT INTO riders (slot, transponder_id, string_id, enabled)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(slot) DO UPDATE SET
                transponder_id = excluded.transponder_id,
                string_id = excluded.string_id,
                enabled = excluded.enabled",
            params![rider.slot, rider.transponder_id, rider.string_id, rider.enabled],
        )?;
        Ok(())
    }

    /// Load settings, filling anything missing with defaults
    pub fn load_settings(&self) -> Result<Settings, ConfigError> {
        let mut settings = Settings::default();
        let mut stmt = self.conn.prepare("SELECT key, value FROM settings")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        for row in rows {
            let (key, value) = row?;
            let invalid = || ConfigError::InvalidSetting {
                key: key.clone(),
                value: value.clone(),
            };
            match key.as_str() {
                "decoder_id" => settings.decoder_id = value.parse().map_err(|_| invalid())?,
                "noise" => settings.noise = value.parse().map_err(|_| invalid())?,
                "temperature_x10" => {
                    settings.temperature_x10 = value.parse().map_err(|_| invalid())?
                }
                "gps_fix" => settings.gps_fix = value.parse().map_err(|_| invalid())?,
                "satellites" => settings.satellites = value.parse().map_err(|_| invalid())?,
                "status_interval_s" => {
                    settings.status_interval_s = value.parse().map_err(|_| invalid())?
                }
                "gate_transponder" => {
                    settings.gate_transponder = value.parse().map_err(|_| invalid())?
                }
                "race_winner_finish_s" => {
                    settings.race_winner_finish_s = value.parse().map_err(|_| invalid())?
                }
                "race_gap_min_s" => {
                    settings.race_gap_min_s = value.parse().map_err(|_| invalid())?
                }
                "race_gap_max_s" => {
                    settings.race_gap_max_s = value.parse().map_err(|_| invalid())?
                }
                _ => {} // Unknown key from a newer version: ignore
            }
        }

        Ok(settings)
    }

    /// Persist all settings
    pub fn save_settings(&self, settings: &Settings) -> Result<(), ConfigError> {
        let pairs: [(&str, String); 10] = [
            ("decoder_id", settings.decoder_id.to_string()),
            ("noise", settings.noise.to_string()),
            ("temperature_x10", settings.temperature_x10.to_string()),
            ("gps_fix", settings.gps_fix.to_string()),
            ("satellites", settings.satellites.to_string()),
            ("status_interval_s", settings.status_interval_s.to_string()),
            ("gate_transponder", settings.gate_transponder.to_string()),
            (
                "race_winner_finish_s",
                settings.race_winner_finish_s.to_string(),
            ),
            ("race_gap_min_s", settings.race_gap_min_s.to_string()),
            ("race_gap_max_s", settings.race_gap_max_s.to_string()),
        ];
        for (key, value) in pairs {
            self.conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_db_seeds_default_riders() {
        let db = Db::open_in_memory().unwrap();
        let riders = db.load_riders().unwrap();
        assert_eq!(riders.len(), 4);
        assert_eq!(riders[0].string_id, "FL-94890");
        assert_eq!(riders[0].transponder_id, 102758186);
        assert!(riders.iter().all(|r| r.string_bytes().is_ok()));
    }

    #[test]
    fn fresh_db_returns_default_settings() {
        let db = Db::open_in_memory().unwrap();
        let settings = db.load_settings().unwrap();
        assert_eq!(settings, Settings::default());
        assert_eq!(settings.decoder_id, 0x000C00D0);
        assert_eq!(settings.gate_transponder, 9992);
    }

    #[test]
    fn settings_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let settings = Settings {
            noise: 62,
            gps_fix: false,
            race_winner_finish_s: 40.0,
            ..Settings::default()
        };

        db.save_settings(&settings).unwrap();
        assert_eq!(db.load_settings().unwrap(), settings);
    }

    #[test]
    fn rider_round_trip_and_update() {
        let db = Db::open_in_memory().unwrap();
        let rider = Rider {
            slot: 5,
            transponder_id: 105555666,
            string_id: "FL-55556".into(),
            enabled: true,
        };
        db.upsert_rider(&rider).unwrap();
        assert_eq!(db.load_riders().unwrap().len(), 5);

        // Update same slot
        let updated = Rider {
            enabled: false,
            ..rider.clone()
        };
        db.upsert_rider(&updated).unwrap();
        let riders = db.load_riders().unwrap();
        assert_eq!(riders.len(), 5);
        assert!(!riders.iter().find(|r| r.slot == 5).unwrap().enabled);
    }

    #[test]
    fn rejects_bad_string_id() {
        let db = Db::open_in_memory().unwrap();
        let rider = Rider {
            slot: 6,
            transponder_id: 1,
            string_id: "TOOLONGSTRING".into(),
            enabled: true,
        };
        assert!(matches!(
            db.upsert_rider(&rider),
            Err(ConfigError::InvalidStringId(_))
        ));
    }

    #[test]
    fn persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let path_str = path.to_str().unwrap();

        {
            let db = Db::open(path_str).unwrap();
            let mut settings = db.load_settings().unwrap();
            settings.noise = 60;
            db.save_settings(&settings).unwrap();
        }

        let db = Db::open(path_str).unwrap();
        assert_eq!(db.load_settings().unwrap().noise, 60);
        // Reopen must not re-seed or duplicate riders
        assert_eq!(db.load_riders().unwrap().len(), 4);
    }
}
