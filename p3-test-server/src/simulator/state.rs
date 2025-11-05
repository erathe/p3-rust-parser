//! Decoder state management

/// Decoder state tracking
#[derive(Debug, Clone)]
pub struct DecoderState {
    /// Decoder serial number
    pub decoder_id: u32,

    /// Sequential passing counter (increments for every detection)
    pub passing_number: u32,

    /// Background noise level (15-65 typical, observed 53-62)
    pub noise_level: u16,

    /// Temperature in tenths of degrees Celsius (e.g., 16 = 1.6°C)
    pub temperature_celsius_x10: i16,

    /// GPS lock status
    pub gps_has_fix: bool,

    /// Number of GPS satellites in use
    pub gps_satellites: u8,
}

impl DecoderState {
    /// Create a new decoder state with default values
    pub fn new(decoder_id: u32) -> Self {
        Self {
            decoder_id,
            passing_number: 0,
            noise_level: 53,
            temperature_celsius_x10: 16,
            gps_has_fix: true,
            gps_satellites: 0,
        }
    }

    /// Increment and return the next passing number
    pub fn next_passing_number(&mut self) -> u32 {
        self.passing_number += 1;
        self.passing_number
    }
}

impl Default for DecoderState {
    fn default() -> Self {
        Self::new(0x000C00D0) // Default decoder ID from live captures
    }
}
