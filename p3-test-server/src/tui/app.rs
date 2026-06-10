//! TUI application state and key handling.

use crate::config::{Db, Rider, Settings};
use crate::faults;
use crate::generator::builder::build_status;
use crate::race::run_race;
use crate::simulator::DecoderSimulator;
use crate::transport::{ClientRegistry, TransportHandle};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rand::Rng;
use std::collections::VecDeque;
use tokio::task::JoinHandle;
use tracing::{info, warn};

const LOG_CAPACITY: usize = 500;

/// Which value an edit-mode row controls
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EditItem {
    Noise,
    Temperature,
    GpsFix,
    Satellites,
    StatusInterval,
    GateTransponder,
    RaceWinnerFinish,
    RaceGapMin,
    RaceGapMax,
    RiderEnabled(usize),
    RiderTransponder(usize),
    RiderString(usize),
}

/// One row in the edit popup
pub struct EditEntry {
    pub label: String,
    pub value: String,
    pub item: EditItem,
    /// Toggled with Space instead of typed
    pub is_toggle: bool,
}

pub enum Mode {
    Normal,
    Edit {
        selected: usize,
        /// Some while typing a new value for the selected row
        input: Option<String>,
    },
}

/// Cached copy of decoder counters for rendering (state lives behind an
/// async mutex the sync render path can't await on)
#[derive(Default, Clone)]
pub struct StateSnapshot {
    pub passing_number: u32,
    pub status_paused: bool,
}

pub struct App {
    pub sim: DecoderSimulator,
    pub handle: TransportHandle,
    pub registry: ClientRegistry,
    pub db: Db,
    pub settings: Settings,
    pub riders: Vec<Rider>,
    pub log: VecDeque<String>,
    pub mode: Mode,
    pub snapshot: StateSnapshot,
    pub port: u16,
    pub chunk_size: Option<usize>,
    pub should_quit: bool,
    race_task: Option<JoinHandle<()>>,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sim: DecoderSimulator,
        handle: TransportHandle,
        registry: ClientRegistry,
        db: Db,
        settings: Settings,
        riders: Vec<Rider>,
        port: u16,
        chunk_size: Option<usize>,
    ) -> Self {
        Self {
            sim,
            handle,
            registry,
            db,
            settings,
            riders,
            log: VecDeque::new(),
            mode: Mode::Normal,
            snapshot: StateSnapshot::default(),
            port,
            chunk_size,
            should_quit: false,
            race_task: None,
        }
    }

    pub fn push_log(&mut self, line: String) {
        if self.log.len() >= LOG_CAPACITY {
            self.log.pop_front();
        }
        self.log.push_back(line);
    }

    /// Refresh cached decoder counters; called on every render tick
    pub fn tick(&mut self) {
        let state_handle = self.sim.state();
        if let Ok(state) = state_handle.try_lock() {
            self.snapshot = StateSnapshot {
                passing_number: state.passing_number,
                status_paused: state.status_paused,
            };
        }
    }

    pub async fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        match &self.mode {
            Mode::Normal => self.handle_normal_key(key).await,
            Mode::Edit { .. } => self.handle_edit_key(key).await,
        }
    }

    async fn handle_normal_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('e') => {
                self.mode = Mode::Edit {
                    selected: 0,
                    input: None,
                };
            }
            KeyCode::Char('g') => {
                if let Err(e) = self
                    .sim
                    .send_gate_passing(self.settings.gate_transponder)
                    .await
                {
                    warn!("Gate passing failed: {}", e);
                } else {
                    info!("Gate drop sent (beacon {})", self.settings.gate_transponder);
                }
            }
            KeyCode::Char(c @ '1'..='8') => {
                let slot = c.to_digit(10).unwrap() as u8;
                self.fire_rider(slot).await;
            }
            KeyCode::Char('r') => self.start_race(),
            KeyCode::Char('s') => {
                if let Err(e) = self.sim.send_status().await {
                    warn!("STATUS send failed: {}", e);
                } else {
                    info!("STATUS sent");
                }
            }
            KeyCode::Char('p') => {
                let state_handle = self.sim.state();
                let paused = {
                    let mut state = state_handle.lock().await;
                    state.status_paused = !state.status_paused;
                    state.status_paused
                };
                info!(
                    "STATUS heartbeat {}",
                    if paused { "PAUSED" } else { "resumed" }
                );
            }
            KeyCode::Char('c') => {
                let frame = faults::corrupt_crc(&self.build_current_status());
                self.send_fault(frame, "corrupt-CRC frame").await;
            }
            KeyCode::Char('x') => {
                let frame = faults::garbage_bytes(64);
                self.send_fault(frame, "64 bytes of garbage").await;
            }
            KeyCode::Char('t') => {
                let frame = faults::truncate_frame(&self.build_current_status());
                self.send_fault(frame, "truncated frame").await;
            }
            _ => {}
        }
    }

    async fn fire_rider(&mut self, slot: u8) {
        let Some(rider) = self.riders.iter().find(|r| r.slot == slot).cloned() else {
            warn!("No rider configured in slot {}", slot);
            return;
        };
        let string = match rider.string_bytes() {
            Ok(s) => s,
            Err(e) => {
                warn!("Rider in slot {} has invalid string: {}", slot, e);
                return;
            }
        };
        let (strength, hits) = {
            let mut rng = rand::thread_rng();
            (rng.gen_range(76..=133), rng.gen_range(2..=33))
        };
        if let Err(e) = self
            .sim
            .send_rider_passing(rider.transponder_id, &string, strength, hits)
            .await
        {
            warn!("Rider passing failed: {}", e);
        } else {
            info!(
                "Passing sent: {} (strength {}, hits {})",
                rider.string_id, strength, hits
            );
        }
    }

    fn start_race(&mut self) {
        if let Some(task) = &self.race_task
            && !task.is_finished()
        {
            warn!("Heat already in progress");
            return;
        }
        self.race_task = Some(tokio::spawn(run_race(
            self.sim.clone(),
            self.riders.clone(),
            self.settings.clone(),
        )));
    }

    /// A STATUS frame reflecting the currently configured decoder values
    fn build_current_status(&self) -> Vec<u8> {
        build_status(
            self.settings.noise,
            self.settings.temperature_x10,
            self.settings.gps_fix as u8,
            self.settings.satellites,
            self.settings.decoder_id,
        )
    }

    async fn send_fault(&mut self, data: Vec<u8>, description: &str) {
        if let Err(e) = self.handle.send(data).await {
            warn!("Fault send failed: {}", e);
        } else {
            info!("FAULT injected: {}", description);
        }
    }

    // --- Edit mode ---

    pub fn edit_entries(&self) -> Vec<EditEntry> {
        let s = &self.settings;
        let mut entries = vec![
            EditEntry {
                label: "Noise".into(),
                value: s.noise.to_string(),
                item: EditItem::Noise,
                is_toggle: false,
            },
            EditEntry {
                label: "Temperature (x10 °C)".into(),
                value: s.temperature_x10.to_string(),
                item: EditItem::Temperature,
                is_toggle: false,
            },
            EditEntry {
                label: "GPS fix".into(),
                value: s.gps_fix.to_string(),
                item: EditItem::GpsFix,
                is_toggle: true,
            },
            EditEntry {
                label: "Satellites".into(),
                value: s.satellites.to_string(),
                item: EditItem::Satellites,
                is_toggle: false,
            },
            EditEntry {
                label: "STATUS interval (s)".into(),
                value: s.status_interval_s.to_string(),
                item: EditItem::StatusInterval,
                is_toggle: false,
            },
            EditEntry {
                label: "Gate beacon ID".into(),
                value: s.gate_transponder.to_string(),
                item: EditItem::GateTransponder,
                is_toggle: false,
            },
            EditEntry {
                label: "Race: winner finish (s)".into(),
                value: format!("{:.1}", s.race_winner_finish_s),
                item: EditItem::RaceWinnerFinish,
                is_toggle: false,
            },
            EditEntry {
                label: "Race: gap min (s)".into(),
                value: format!("{:.1}", s.race_gap_min_s),
                item: EditItem::RaceGapMin,
                is_toggle: false,
            },
            EditEntry {
                label: "Race: gap max (s)".into(),
                value: format!("{:.1}", s.race_gap_max_s),
                item: EditItem::RaceGapMax,
                is_toggle: false,
            },
        ];

        for (i, rider) in self.riders.iter().enumerate() {
            entries.push(EditEntry {
                label: format!("Rider {} enabled", rider.slot),
                value: rider.enabled.to_string(),
                item: EditItem::RiderEnabled(i),
                is_toggle: true,
            });
            entries.push(EditEntry {
                label: format!("Rider {} transponder", rider.slot),
                value: rider.transponder_id.to_string(),
                item: EditItem::RiderTransponder(i),
                is_toggle: false,
            });
            entries.push(EditEntry {
                label: format!("Rider {} string (8 chars)", rider.slot),
                value: rider.string_id.clone(),
                item: EditItem::RiderString(i),
                is_toggle: false,
            });
        }

        entries
    }

    async fn handle_edit_key(&mut self, key: KeyEvent) {
        let entries = self.edit_entries();
        let Mode::Edit { selected, input } = &mut self.mode else {
            return;
        };

        match input {
            // Typing a new value
            Some(buffer) => match key.code {
                KeyCode::Esc => *input = None,
                KeyCode::Backspace => {
                    buffer.pop();
                }
                KeyCode::Enter => {
                    let value = buffer.clone();
                    let item = entries[*selected].item;
                    *input = None;
                    self.commit_edit(item, &value).await;
                }
                KeyCode::Char(c) => buffer.push(c),
                _ => {}
            },
            // Navigating
            None => match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::Normal,
                KeyCode::Up | KeyCode::Char('k') => {
                    *selected = selected.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    *selected = (*selected + 1).min(entries.len() - 1);
                }
                KeyCode::Enter => {
                    let entry = &entries[*selected];
                    if entry.is_toggle {
                        let item = entry.item;
                        self.toggle_edit(item).await;
                    } else {
                        *input = Some(String::new());
                    }
                }
                KeyCode::Char(' ') => {
                    let entry = &entries[*selected];
                    if entry.is_toggle {
                        let item = entry.item;
                        self.toggle_edit(item).await;
                    }
                }
                _ => {}
            },
        }
    }

    async fn toggle_edit(&mut self, item: EditItem) {
        match item {
            EditItem::GpsFix => {
                self.settings.gps_fix = !self.settings.gps_fix;
                self.apply_and_persist().await;
            }
            EditItem::RiderEnabled(i) => {
                self.riders[i].enabled = !self.riders[i].enabled;
                self.persist_rider(i);
            }
            _ => {}
        }
    }

    async fn commit_edit(&mut self, item: EditItem, value: &str) {
        macro_rules! parse_or_warn {
            ($ty:ty) => {
                match value.parse::<$ty>() {
                    Ok(v) => v,
                    Err(_) => {
                        warn!("Invalid value {:?}", value);
                        return;
                    }
                }
            };
        }

        match item {
            EditItem::Noise => self.settings.noise = parse_or_warn!(u16),
            EditItem::Temperature => self.settings.temperature_x10 = parse_or_warn!(i16),
            EditItem::Satellites => self.settings.satellites = parse_or_warn!(u8),
            EditItem::StatusInterval => {
                self.settings.status_interval_s = parse_or_warn!(u64).max(1)
            }
            EditItem::GateTransponder => self.settings.gate_transponder = parse_or_warn!(u32),
            EditItem::RaceWinnerFinish => {
                self.settings.race_winner_finish_s = parse_or_warn!(f64).max(0.1)
            }
            EditItem::RaceGapMin => self.settings.race_gap_min_s = parse_or_warn!(f64).max(0.0),
            EditItem::RaceGapMax => {
                self.settings.race_gap_max_s =
                    parse_or_warn!(f64).max(self.settings.race_gap_min_s)
            }
            EditItem::GpsFix | EditItem::RiderEnabled(_) => {} // toggle-only
            EditItem::RiderTransponder(i) => {
                self.riders[i].transponder_id = parse_or_warn!(u32);
                self.persist_rider(i);
                return;
            }
            EditItem::RiderString(i) => {
                if value.len() != 8 || !value.is_ascii() {
                    warn!("Rider string must be exactly 8 ASCII chars, got {:?}", value);
                    return;
                }
                self.riders[i].string_id = value.to_string();
                self.persist_rider(i);
                return;
            }
        }
        self.apply_and_persist().await;
    }

    /// Push current settings into the live decoder state and save to SQLite
    async fn apply_and_persist(&mut self) {
        {
            let state_handle = self.sim.state();
            let mut state = state_handle.lock().await;
            state.decoder_id = self.settings.decoder_id;
            state.noise_level = self.settings.noise;
            state.temperature_celsius_x10 = self.settings.temperature_x10;
            state.gps_has_fix = self.settings.gps_fix;
            state.gps_satellites = self.settings.satellites;
            state.status_interval_s = self.settings.status_interval_s;
        }
        match self.db.save_settings(&self.settings) {
            Ok(()) => info!("Settings applied and saved"),
            Err(e) => warn!("Failed to save settings: {}", e),
        }
    }

    fn persist_rider(&mut self, index: usize) {
        match self.db.upsert_rider(&self.riders[index]) {
            Ok(()) => info!("Rider {} saved", self.riders[index].slot),
            Err(e) => warn!("Failed to save rider: {}", e),
        }
    }
}
