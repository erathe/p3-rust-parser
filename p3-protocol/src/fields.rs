/// TLV (Tag-Length-Value) field tags used in P3 messages
///
/// Each message body contains multiple fields encoded as:
/// [Tag: 1 byte][Length: 1 byte][Value: N bytes]

/// PASSING message field tags
///
/// These tags are validated against live capture data from MyLaps ProChip
/// decoder (2025-10-30).
pub mod passing {
    /// Sequential counter (u32) - increments with each detection
    pub const PASSING_NUMBER: u8 = 0x01;

    /// Transponder chip ID (u32)
    pub const TRANSPONDER: u8 = 0x03;

    /// Internal RTC time (u64) - microseconds since Unix epoch
    pub const RTC_TIME: u8 = 0x04;

    /// Signal strength (u16) - typical range 60-150
    /// Only present in rider transponder messages
    pub const STRENGTH: u8 = 0x05;

    /// Number of signal hits detected (u16) - typical range 30-50
    /// Only present in rider transponder messages
    pub const HITS: u8 = 0x06;

    /// Passing flags (u16)
    pub const FLAGS: u8 = 0x08;

    /// Transponder string identifier (8 bytes ASCII)
    /// Example: "FL-94890" (used for rider mapping)
    /// Only present in rider transponder messages (not in gate pulses)
    pub const STRING: u8 = 0x0A;

    /// GPS-synchronized UTC time (u64) - microseconds since Unix epoch
    /// Only present when GPS has a fix
    /// NOTE: Not observed in live capture, may be firmware-dependent
    pub const UTC_TIME: u8 = 0x10;

    /// Decoder ID / Serial Number (u32)
    /// Identifies which decoder sent this message. Format: little-endian bytes
    /// representing hex string (e.g., 0x000C00D0 = "D0000C00").
    /// Essential for multi-decoder timing systems (start line, finish line, splits).
    /// Value 0x000C00D0 observed consistently in live capture from decoder "D0000C00"
    pub const DECODER_ID: u8 = 0x81;
}

/// STATUS message field tags
///
/// These tags are validated against live capture data from MyLaps ProChip
/// decoder (2025-10-30). All 88 STATUS messages captured used these exact tags.
pub mod status {
    /// Background noise level (u16) - typical range 15-60
    /// Observed values: 53-62 in live capture
    pub const NOISE: u8 = 0x01;

    /// GPS status byte (u8)
    /// 0 = no fix, 1 = GPS locked
    pub const GPS_STATUS: u8 = 0x06;

    /// Temperature in tenths of degrees Celsius (i16)
    /// Example: 16 = 1.6°C (divide by 10)
    /// Observed range: 15-16 (1.5-1.6°C) in live capture
    pub const TEMPERATURE: u8 = 0x07;

    /// Number of GPS satellites in use (u8)
    /// Observed as 0 even when GPS_STATUS = 1 (may indicate indoor use)
    pub const SATINUSE: u8 = 0x0A;

    /// Decoder ID / Serial Number (u32)
    /// Identifies which decoder sent this message. Format: little-endian bytes
    /// representing hex string (e.g., 0x000C00D0 = "D0000C00").
    /// Essential for multi-decoder timing systems (start line, finish line, splits).
    /// Value 0x000C00D0 observed consistently in all STATUS messages from decoder "D0000C00"
    pub const DECODER_ID: u8 = 0x81;
}

/// VERSION message field tags
///
/// NOTE: Not validated against live capture. Based on community documentation.
pub mod version {
    /// Decoder ID / Registration number (u64)
    pub const DECODER_ID: u8 = 0x20;

    /// Device description string (variable length)
    pub const DESCRIPTION: u8 = 0x21;

    /// Firmware version string (variable length)
    pub const VERSION: u8 = 0x22;

    /// Build number (u16)
    pub const BUILD: u8 = 0x23;
}

/// DEPRECATED: Field tags from community documentation that DO NOT match real decoders
///
/// **⚠️ WARNING: DO NOT USE THESE TAGS ⚠️**
///
/// These tag values were found in community reverse-engineering documentation
/// (HobbyTalk forums, GitHub implementations) but do NOT match the tags observed
/// in live capture data from actual MyLaps ProChip decoders.
///
/// These constants are preserved for historical reference only and to help
/// anyone debugging parsers based on the incorrect community documentation.
///
/// **Use the correct tags from `status` module instead.**
///
/// See: tests/fixtures/live_capture/README.md for validation details
#[deprecated(
    since = "0.1.0",
    note = "These tags do not match real MyLaps decoders. Use `status` module tags instead."
)]
pub mod deprecated_status_tags {
    /// INCORRECT: Community docs say NOISE = 0x15, but real decoders use 0x01
    pub const NOISE_WRONG: u8 = 0x15;

    /// INCORRECT: Community docs say GPS_STATUS = 0x16, but real decoders use 0x06
    pub const GPS_STATUS_WRONG: u8 = 0x16;

    /// INCORRECT: Community docs say TEMPERATURE = 0x17, but real decoders use 0x07
    pub const TEMPERATURE_WRONG: u8 = 0x17;

    /// INCORRECT: Community docs say SATELLITES = 0x18, but real decoders use 0x0A
    pub const SATELLITES_WRONG: u8 = 0x18;

    /// INCORRECT: Community docs say VOLTAGE = 0x19, not observed in live capture
    pub const VOLTAGE_WRONG: u8 = 0x19;

    /// INCORRECT: Community docs say LOOP_TRIGGERS = 0x1A, not observed in live capture
    pub const LOOP_TRIGGERS_WRONG: u8 = 0x1A;
}

/// Reserved transponder IDs for system use
///
/// These transponder IDs are used by gate timing beacons rather than rider
/// transponders. Validated in live capture (2025-10-30): IDs 9992 and 9995
/// observed as gate start signals.
pub mod reserved_ids {
    /// Gate drop signal for 5-meter hill tracks
    /// Not observed in live capture, from community documentation
    pub const GATE_DROP_5M: u32 = 9991;

    /// Gate drop signal for 8-meter hill tracks
    /// Confirmed in live capture: 6 gate messages with this ID
    pub const GATE_DROP_8M: u32 = 9992;

    /// Alternative gate drop signal
    /// Confirmed in live capture: 1 gate message with this ID
    pub const GATE_DROP_OTHER: u32 = 9995;

    /// Check if a transponder ID is reserved for system use (gate beacon)
    pub const fn is_reserved(id: u32) -> bool {
        id == GATE_DROP_5M || id == GATE_DROP_8M || id == GATE_DROP_OTHER
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reserved_ids() {
        assert!(reserved_ids::is_reserved(9991));
        assert!(reserved_ids::is_reserved(9992));
        assert!(reserved_ids::is_reserved(9995));
        assert!(!reserved_ids::is_reserved(1234567));
        assert!(!reserved_ids::is_reserved(102758186)); // Rider transponder from live capture
    }
}
