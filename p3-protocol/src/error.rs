use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum EscapeError {
    #[error("Incomplete escape sequence: escape byte at end of data")]
    IncompleteSequence,

    #[error("Invalid escape sequence: 0x8D followed by 0x{0:02X}")]
    InvalidSequence(u8),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CrcError {
    #[error("CRC validation failed: expected 0x{expected:04X}, got 0x{actual:04X}")]
    ValidationFailed { expected: u16, actual: u16 },

    #[error("Data too short to contain CRC (length: {0})")]
    DataTooShort(usize),

    #[error(
        "Malformed escape sequence at position {position}: 0x8D followed by 0x{next_byte:02X} (expected 0xAA-0xAF)"
    )]
    MalformedEscape { position: usize, next_byte: u8 },

    #[error("Message too short: {actual} bytes (minimum {min} bytes required)")]
    MessageTooShort { actual: usize, min: usize },
}
