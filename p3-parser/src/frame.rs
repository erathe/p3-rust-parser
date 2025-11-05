use crate::error::{ParseError, ParseResult};
use p3_protocol::{
    EOR, MIN_FRAME_SIZE, MessageType, OFFSET_BODY, OFFSET_CRC, OFFSET_LENGTH, OFFSET_RESERVED,
    OFFSET_SOR, OFFSET_TYPE, OFFSET_VERSION, SOR, VERSION, unescape_data, validate_crc,
};

/// A parsed P3 message frame
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub message_type: MessageType,
    /// Unescaped message body (TLV fields)
    pub body: Vec<u8>,
    pub crc: u16,
    pub reserved: u16,
}

pub struct FrameParser {
    // TODO: Add buffer management for stream parsing
}

impl FrameParser {
    pub fn new() -> Self {
        Self {}
    }

    /// Parse a single frame from binary data
    ///
    /// The input data should be the complete escaped message including SOR and EOR.
    pub fn parse(&self, data: &[u8]) -> ParseResult<Frame> {
        // Minimum frame size: SOR + VERSION + LEN + CRC + RES + TYPE + EOR
        if data.len() < MIN_FRAME_SIZE {
            return Err(ParseError::IncompleteMessage {
                expected: MIN_FRAME_SIZE,
                actual: data.len(),
            });
        }

        // Validate SOR marker
        if data[OFFSET_SOR] != SOR {
            return Err(ParseError::InvalidFrame("Missing SOR marker".into()));
        }

        // Validate version
        if data[OFFSET_VERSION] != VERSION {
            return Err(ParseError::InvalidFrame(format!(
                "Unsupported version: 0x{:02X}",
                data[OFFSET_VERSION]
            )));
        }

        // Validate CRC (validates the complete escaped message)
        validate_crc(data).map_err(|e| ParseError::CrcError(e))?;

        // Unescape the data after CRC validation
        let unescaped = unescape_data(data).map_err(|e| ParseError::EscapeError(e))?;

        // Now work with unescaped data
        // Header: SOR(1) + VER(1) + LEN(2) + CRC(2) + RES(2) + TYPE(2) = 10 bytes
        if unescaped.len() < MIN_FRAME_SIZE {
            // Need at least header + EOR
            return Err(ParseError::IncompleteMessage {
                expected: MIN_FRAME_SIZE,
                actual: unescaped.len(),
            });
        }

        // Extract header fields (all little-endian)
        let length = u16::from_le_bytes([unescaped[OFFSET_LENGTH], unescaped[OFFSET_LENGTH + 1]]);
        let crc = u16::from_le_bytes([unescaped[OFFSET_CRC], unescaped[OFFSET_CRC + 1]]);
        let reserved =
            u16::from_le_bytes([unescaped[OFFSET_RESERVED], unescaped[OFFSET_RESERVED + 1]]);
        let message_type_raw =
            u16::from_le_bytes([unescaped[OFFSET_TYPE], unescaped[OFFSET_TYPE + 1]]);

        let message_type = MessageType::from_u16(message_type_raw)
            .ok_or_else(|| ParseError::UnknownMessageType(message_type_raw))?;

        // Validate length matches actual data
        // Length field does NOT include escape bytes, and represents unescaped length
        if unescaped.len() != length as usize {
            return Err(ParseError::InvalidFrame(format!(
                "Length mismatch: header says {}, got {}",
                length,
                unescaped.len()
            )));
        }

        // Validate EOR marker at the end
        let eor_pos = unescaped.len() - 1;
        if unescaped[eor_pos] != EOR {
            return Err(ParseError::InvalidFrame("Missing EOR marker".into()));
        }

        // Extract body (between header and EOR)
        // Body starts at OFFSET_BODY (after header) and ends before EOR
        let body = unescaped[OFFSET_BODY..eor_pos].to_vec();

        Ok(Frame {
            message_type,
            body,
            crc,
            reserved,
        })
    }
}

impl Default for FrameParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore]
    fn test_parse_frame() {
        // TODO: Add tests
    }
}
