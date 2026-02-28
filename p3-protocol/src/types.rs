/// P3 Protocol Constants
///
/// These define the frame structure and control bytes used in the
/// MyLaps ProChip P3 binary protocol.

/// Start of Record - marks the beginning of a message
pub const SOR: u8 = 0x8E;

/// End of Record - marks the end of a message
pub const EOR: u8 = 0x8F;

/// Escape byte - used to escape control bytes in message data
pub const ESCAPE: u8 = 0x8D;

/// Protocol version - always 0x02 for P3
pub const VERSION: u8 = 0x02;

/// Range of control bytes that must be escaped (0x8A through 0x8F)
pub const ESCAPE_RANGE_START: u8 = 0x8A;
pub const ESCAPE_RANGE_END: u8 = 0x8F;

/// Value added to escaped bytes (escape mechanism)
pub const ESCAPE_OFFSET: u8 = 0x20;

// Frame Structure Constants
//
// P3 Frame Layout (unescaped):
// ┌────────┬─────────┬────────┬─────┬──────────┬──────┬──────────┬────────┐
// │  SOR   │ VERSION │ LENGTH │ CRC │ RESERVED │ TYPE │   BODY   │  EOR   │
// │  0x8E  │  0x02   │ 2 bytes│2 byt│ 2 bytes  │2 byt │ Variable │  0x8F  │
// └────────┴─────────┴────────┴─────┴──────────┴──────┴──────────┴────────┘

/// Byte offset of SOR field in unescaped frame
pub const OFFSET_SOR: usize = 0;

/// Byte offset of VERSION field in unescaped frame
pub const OFFSET_VERSION: usize = 1;

/// Byte offset of LENGTH field in unescaped frame (2 bytes, little-endian)
pub const OFFSET_LENGTH: usize = 2;

/// Byte offset of CRC field in unescaped frame (2 bytes, little-endian)
pub const OFFSET_CRC: usize = 4;

/// Byte offset of RESERVED field in unescaped frame (2 bytes, little-endian)
pub const OFFSET_RESERVED: usize = 6;

/// Byte offset of TYPE (TOR) field in unescaped frame (2 bytes, little-endian)
pub const OFFSET_TYPE: usize = 8;

/// Byte offset where BODY starts in unescaped frame
pub const OFFSET_BODY: usize = 10;

/// Size of a u16 field in bytes
pub const SIZE_U16_FIELD: usize = 2;

/// Total size of frame header (SOR + VERSION + LENGTH + CRC + RESERVED + TYPE)
pub const HEADER_SIZE: usize = 10;

/// Minimum valid frame size (HEADER + EOR)
pub const MIN_FRAME_SIZE: usize = 11;

/// Type of Record (TOR) values - identifies message type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum MessageType {
    /// Transponder detection with timing data
    Passing = 0x0001,

    /// Decoder operational status
    Status = 0x0002,

    /// Hardware/firmware identification
    Version = 0x0003,

    /// Request to retransmit data
    Resend = 0x0004,
}

impl MessageType {
    pub fn to_u16(self) -> u16 {
        self as u16
    }

    pub fn from_u16(value: u16) -> Option<Self> {
        match value {
            0x0001 => Some(MessageType::Passing),
            0x0002 => Some(MessageType::Status),
            0x0003 => Some(MessageType::Version),
            0x0004 => Some(MessageType::Resend),
            _ => None,
        }
    }
}

impl From<MessageType> for u16 {
    fn from(msg_type: MessageType) -> Self {
        msg_type.to_u16()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidMessageType(pub u16);

impl std::fmt::Display for InvalidMessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Invalid message type: 0x{:04X}", self.0)
    }
}

impl std::error::Error for InvalidMessageType {}

impl TryFrom<u16> for MessageType {
    type Error = InvalidMessageType;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        MessageType::from_u16(value).ok_or(InvalidMessageType(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_control_bytes() {
        assert_eq!(SOR, 0x8E);
        assert_eq!(EOR, 0x8F);
        assert_eq!(ESCAPE, 0x8D);
    }

    #[test]
    fn test_frame_offsets() {
        assert_eq!(OFFSET_SOR, 0);
        assert_eq!(OFFSET_VERSION, 1);
        assert_eq!(OFFSET_LENGTH, 2);
        assert_eq!(OFFSET_CRC, 4);
        assert_eq!(OFFSET_RESERVED, 6);
        assert_eq!(OFFSET_TYPE, 8);
        assert_eq!(OFFSET_BODY, 10);
    }

    #[test]
    fn test_frame_sizes() {
        assert_eq!(SIZE_U16_FIELD, 2);
        assert_eq!(HEADER_SIZE, 10);
        assert_eq!(MIN_FRAME_SIZE, 11);

        // Verify header size calculation
        assert_eq!(
            HEADER_SIZE, OFFSET_BODY,
            "HEADER_SIZE should equal OFFSET_BODY"
        );
    }

    #[test]
    fn test_message_type_conversion() {
        assert_eq!(MessageType::Passing.to_u16(), 0x0001);
        assert_eq!(MessageType::Status.to_u16(), 0x0002);
        assert_eq!(MessageType::Version.to_u16(), 0x0003);
        assert_eq!(MessageType::Resend.to_u16(), 0x0004);

        assert_eq!(MessageType::from_u16(0x0001), Some(MessageType::Passing));
        assert_eq!(MessageType::from_u16(0x0002), Some(MessageType::Status));
        assert_eq!(MessageType::from_u16(0x9999), None);
    }

    #[test]
    fn test_message_type_from_trait() {
        let value: u16 = MessageType::Passing.into();
        assert_eq!(value, 0x0001);
    }

    #[test]
    fn test_message_type_try_from_valid() {
        use std::convert::TryFrom;

        assert_eq!(MessageType::try_from(0x0001).unwrap(), MessageType::Passing);
        assert_eq!(MessageType::try_from(0x0002).unwrap(), MessageType::Status);
        assert_eq!(MessageType::try_from(0x0003).unwrap(), MessageType::Version);
        assert_eq!(MessageType::try_from(0x0004).unwrap(), MessageType::Resend);
    }

    #[test]
    fn test_message_type_try_from_invalid() {
        use std::convert::TryFrom;

        let result = MessageType::try_from(0x9999);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), InvalidMessageType(0x9999));
    }
}
