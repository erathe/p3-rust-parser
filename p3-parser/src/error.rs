use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Invalid frame: {0}")]
    InvalidFrame(String),

    #[error("CRC validation failed")]
    CrcError(#[from] p3_protocol::CrcError),

    #[error("Escape sequence error")]
    EscapeError(#[from] p3_protocol::EscapeError),

    #[error("TLV parsing error: {0}")]
    TlvError(String),

    #[error("Unknown message type: 0x{0:04X}")]
    UnknownMessageType(u16),

    #[error("Incomplete message: expected {expected} bytes, got {actual}")]
    IncompleteMessage { expected: usize, actual: usize },

    #[error("IO error")]
    IoError(#[from] std::io::Error),
}

pub type ParseResult<T> = Result<T, ParseError>;
