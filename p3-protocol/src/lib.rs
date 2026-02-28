//! # P3 Protocol - MyLaps ProChip Core Library
//!
//! Low-level protocol implementation for the MyLaps ProChip P3 binary protocol.
//!
//! ## What This Library Provides
//!
//! - **Frame constants** (SOR, EOR, ESCAPE)
//! - **Message types** (PASSING, STATUS, VERSION, RESEND)
//! - **TLV field definitions** for all message types
//! - **Escape/unescape functions** for control byte handling
//! - **CRC calculation and validation** (exact decoder algorithm)
//!
//! ## What This Library Does NOT Provide
//!
//! - Message parsing (see `p3-parser` crate)
//! - Message generation (see `p3-test-server` crate)
//! - I/O operations (TCP/serial)
//!
//! This is a pure logic library with zero I/O dependencies.
//!
//! ## Example Usage
//!
//! ```rust
//! use p3_protocol::{MessageType, SOR, EOR, ESCAPE};
//! use p3_protocol::fields::passing;
//!
//! // Access protocol constants
//! assert_eq!(SOR, 0x8E);
//! assert_eq!(EOR, 0x8F);
//!
//! // Use message types
//! let msg_type = MessageType::Passing;
//! assert_eq!(msg_type.to_u16(), 0x0001);
//!
//! // Access field tags
//! assert_eq!(passing::TRANSPONDER, 0x03);
//! ```

pub mod crc;
pub mod error;
pub mod escape;
pub mod fields;
pub mod types;

// Re-export commonly used items at crate root
pub use crc::{calculate_crc, calculate_message_crc, validate_crc};
pub use error::*;
pub use escape::{
    EscapeInfo, encode, escape_data, escaped_length, unescape_data, unescaped_length,
};
pub use types::*;
