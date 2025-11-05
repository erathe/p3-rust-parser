//! # P3 Parser - MyLaps ProChip P3 Protocol Parser
//!
//! Parse binary P3 protocol messages to structured data and JSON.
//!
//! ## Overview
//!
//! This library parses binary messages from MyLaps ProChip Smart Decoders
//! and converts them to structured Rust types or JSON format.
//!
//! ## Message Types
//!
//! - **PASSING** - Transponder detection with timing data
//! - **STATUS** - Decoder operational status
//! - **VERSION** - Hardware/firmware identification
//!
//! ## Example Usage
//!
//! ```rust,ignore
//! use p3_parser::Parser;
//!
//! // Parse a binary message
//! let data = vec![0x8E, 0x02, /* ... */];
//! let parser = Parser::new();
//! let message = parser.parse(&data)?;
//!
//! // Convert to JSON
//! let json = serde_json::to_string(&message)?;

pub mod error;
pub mod frame;
pub mod messages;
pub mod tlv;

pub use error::*;
pub use frame::*;
pub use messages::*;
pub use tlv::*;

use p3_protocol::MessageType;

pub struct Parser {
    frame_parser: FrameParser,
    tlv_decoder: TlvDecoder,
}

impl Parser {
    pub fn new() -> Self {
        Self {
            frame_parser: FrameParser::new(),
            tlv_decoder: TlvDecoder::new(),
        }
    }

    /// The input should be a complete escaped message including SOR and EOR markers.
    pub fn parse(&self, data: &[u8]) -> ParseResult<Message> {
        let frame = self.frame_parser.parse(data)?;

        let fields = self.tlv_decoder.decode(&frame.body)?;

        let message = match frame.message_type {
            MessageType::Passing => Message::Passing(PassingMessage::from_tlv_fields(&fields)?),
            MessageType::Status => Message::Status(StatusMessage::from_tlv_fields(&fields)?),
            MessageType::Version => Message::Version(VersionMessage::from_tlv_fields(&fields)?),
            MessageType::Resend => {
                return Err(ParseError::UnknownMessageType(MessageType::Resend.to_u16()));
            }
        };

        Ok(message)
    }
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}
