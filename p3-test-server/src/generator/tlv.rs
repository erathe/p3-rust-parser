//! TLV (Tag-Length-Value) encoding for P3 protocol fields.
//!
//! The P3 protocol uses TLV encoding for message payloads:
//! - Tag: 1 byte (field identifier)
//! - Length: 1 byte (number of value bytes)
//! - Value: N bytes (little-endian for multi-byte integers)
//!
//! # Example
//! ```
//! use p3_test_server::generator::tlv::TlvBuilder;
//!
//! let tlv = TlvBuilder::new()
//!     .add_u32(0x01, 12345)
//!     .add_u16(0x05, 127)
//!     .build();
//! ```

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TlvError {
    /// Value length exceeds maximum of 255 bytes
    #[error("TLV value length {actual} exceeds maximum of {max} bytes")]
    ValueTooLong { actual: usize, max: usize },
}

/// Encode a u8 value as TLV.
///
/// Format: [tag: 1 byte][length: 1][value: 1 byte]
pub fn encode_u8(tag: u8, value: u8) -> Vec<u8> {
    vec![tag, 1, value]
}

/// Encode a u16 value as TLV in little-endian format.
///
/// Format: [tag: 1 byte][length: 2][value: 2 bytes LE]
pub fn encode_u16(tag: u8, value: u16) -> Vec<u8> {
    let mut result = vec![tag, 2];
    result.extend_from_slice(&value.to_le_bytes());
    result
}

/// Encode an i16 value as TLV in little-endian format.
///
/// Format: [tag: 1 byte][length: 2][value: 2 bytes LE]
pub fn encode_i16(tag: u8, value: i16) -> Vec<u8> {
    let mut result = vec![tag, 2];
    result.extend_from_slice(&value.to_le_bytes());
    result
}

/// Encode a u32 value as TLV in little-endian format.
///
/// Format: [tag: 1 byte][length: 4][value: 4 bytes LE]
pub fn encode_u32(tag: u8, value: u32) -> Vec<u8> {
    let mut result = vec![tag, 4];
    result.extend_from_slice(&value.to_le_bytes());
    result
}

/// Encode a u64 value as TLV in little-endian format.
///
/// Format: [tag: 1 byte][length: 8][value: 8 bytes LE]
pub fn encode_u64(tag: u8, value: u64) -> Vec<u8> {
    let mut result = vec![tag, 8];
    result.extend_from_slice(&value.to_le_bytes());
    result
}

/// Encode a byte slice as TLV.
///
/// Format: [tag: 1 byte][length: N][value: N bytes]
///
/// # Errors
/// Returns `TlvError::ValueTooLong` if the byte slice length exceeds 255 bytes.
pub fn encode_bytes(tag: u8, value: &[u8]) -> Result<Vec<u8>, TlvError> {
    if value.len() > 255 {
        return Err(TlvError::ValueTooLong {
            actual: value.len(),
            max: 255,
        });
    }
    let mut result = vec![tag, value.len() as u8];
    result.extend_from_slice(value);
    Ok(result)
}

/// Builder for constructing TLV-encoded message bodies.
///
/// Provides a fluent API for chaining multiple field additions.
///
/// # Example
/// ```
/// use p3_test_server::generator::tlv::TlvBuilder;
///
/// let body = TlvBuilder::new()
///     .add_u32(0x01, 8841)           // PASSING_NUMBER
///     .add_u32(0x03, 102758186)      // TRANSPONDER
///     .add_u16(0x05, 127)            // STRENGTH
///     .add_u16(0x06, 33)             // HITS
///     .build();
/// ```
#[derive(Debug, Default)]
pub struct TlvBuilder {
    data: Vec<u8>,
}

impl TlvBuilder {
    /// Create a new empty TLV builder.
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }

    /// Add a u8 field.
    pub fn add_u8(mut self, tag: u8, value: u8) -> Self {
        self.data.extend_from_slice(&encode_u8(tag, value));
        self
    }

    /// Add a u16 field (little-endian).
    pub fn add_u16(mut self, tag: u8, value: u16) -> Self {
        self.data.extend_from_slice(&encode_u16(tag, value));
        self
    }

    /// Add an i16 field (little-endian).
    pub fn add_i16(mut self, tag: u8, value: i16) -> Self {
        self.data.extend_from_slice(&encode_i16(tag, value));
        self
    }

    /// Add a u32 field (little-endian).
    pub fn add_u32(mut self, tag: u8, value: u32) -> Self {
        self.data.extend_from_slice(&encode_u32(tag, value));
        self
    }

    /// Add a u64 field (little-endian).
    pub fn add_u64(mut self, tag: u8, value: u64) -> Self {
        self.data.extend_from_slice(&encode_u64(tag, value));
        self
    }

    /// Add a byte slice field.
    ///
    /// # Errors
    /// Returns `TlvError::ValueTooLong` if the byte slice length exceeds 255 bytes.
    pub fn add_bytes(mut self, tag: u8, value: &[u8]) -> Result<Self, TlvError> {
        let encoded = encode_bytes(tag, value)?;
        self.data.extend_from_slice(&encoded);
        Ok(self)
    }

    /// Build and return the complete TLV-encoded body.
    pub fn build(self) -> Vec<u8> {
        self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_u8() {
        let result = encode_u8(0x06, 1);
        assert_eq!(result, vec![0x06, 0x01, 0x01]);
    }

    #[test]
    fn test_encode_u16() {
        // Test little-endian encoding
        let result = encode_u16(0x05, 0x7F00);
        assert_eq!(result, vec![0x05, 0x02, 0x00, 0x7F]);
    }

    #[test]
    fn test_encode_u16_simple() {
        let result = encode_u16(0x05, 127);
        assert_eq!(result, vec![0x05, 0x02, 0x7F, 0x00]);
    }

    #[test]
    fn test_encode_i16_positive() {
        let result = encode_i16(0x07, 16);
        assert_eq!(result, vec![0x07, 0x02, 0x10, 0x00]);
    }

    #[test]
    fn test_encode_i16_negative() {
        let result = encode_i16(0x07, -10);
        assert_eq!(result, vec![0x07, 0x02, 0xF6, 0xFF]);
    }

    #[test]
    fn test_encode_u32() {
        // Test with value from live capture: passing number 8841
        let result = encode_u32(0x01, 8841);
        assert_eq!(result, vec![0x01, 0x04, 0x89, 0x22, 0x00, 0x00]);
    }

    #[test]
    fn test_encode_u32_transponder() {
        // Test with transponder ID from live capture: 102758186
        let result = encode_u32(0x03, 102758186);
        assert_eq!(result, vec![0x03, 0x04, 0x2A, 0xF7, 0x1F, 0x06]);
    }

    #[test]
    fn test_encode_u64() {
        // Test with timestamp from live capture
        let result = encode_u64(0x04, 0x0006426530063546);
        assert_eq!(
            result,
            vec![0x04, 0x08, 0x46, 0x35, 0x06, 0x30, 0x65, 0x42, 0x06, 0x00]
        );
    }

    #[test]
    fn test_encode_bytes() {
        let string_data = b"FL-94890";
        let result = encode_bytes(0x0A, string_data).unwrap();
        assert_eq!(
            result,
            vec![0x0A, 0x08, b'F', b'L', b'-', b'9', b'4', b'8', b'9', b'0']
        );
    }

    #[test]
    fn test_encode_bytes_too_long() {
        let long_data = vec![0u8; 256];
        let result = encode_bytes(0x01, &long_data);
        assert!(matches!(
            result,
            Err(TlvError::ValueTooLong {
                actual: 256,
                max: 255
            })
        ));
    }

    #[test]
    fn test_builder_add_bytes_too_long() {
        let long_data = vec![0u8; 256];
        let result = TlvBuilder::new().add_bytes(0x01, &long_data);
        assert!(matches!(
            result,
            Err(TlvError::ValueTooLong {
                actual: 256,
                max: 255
            })
        ));
    }

    #[test]
    fn test_tlv_builder_chain() {
        let result = TlvBuilder::new()
            .add_u32(0x01, 8841)
            .add_u32(0x03, 102758186)
            .add_u16(0x05, 127)
            .add_u16(0x06, 33)
            .build();

        let expected = vec![
            0x01, 0x04, 0x89, 0x22, 0x00, 0x00, // PASSING_NUMBER
            0x03, 0x04, 0x2A, 0xF7, 0x1F, 0x06, // TRANSPONDER
            0x05, 0x02, 0x7F, 0x00, // STRENGTH
            0x06, 0x02, 0x21, 0x00, // HITS
        ];
        assert_eq!(result, expected);
    }

    #[test]
    fn test_tlv_builder_with_bytes() {
        let result = TlvBuilder::new()
            .add_u32(0x01, 8841)
            .add_bytes(0x0A, b"FL-94890")
            .unwrap()
            .build();

        let expected = vec![
            0x01, 0x04, 0x89, 0x22, 0x00, 0x00, // PASSING_NUMBER
            0x0A, 0x08, b'F', b'L', b'-', b'9', b'4', b'8', b'9', b'0', // STRING
        ];
        assert_eq!(result, expected);
    }

    #[test]
    fn test_tlv_builder_status_message() {
        // Build TLV body from live capture: captured_message_001.bin
        let result = TlvBuilder::new()
            .add_u16(0x01, 53) // NOISE
            .add_i16(0x07, 16) // TEMPERATURE (1.6°C)
            .add_u8(0x06, 1) // GPS_STATUS
            .add_u8(0x0A, 0) // Unknown field
            .add_u32(0x81, 0x000C00D0) // DECODER_ID (D0000C00)
            .build();

        let expected = vec![
            0x01, 0x02, 0x35, 0x00, // NOISE: 53
            0x07, 0x02, 0x10, 0x00, // TEMPERATURE: 16 (1.6°C)
            0x06, 0x01, 0x01, // GPS_STATUS: 1
            0x0A, 0x01, 0x00, // Unknown: 0
            0x81, 0x04, 0xD0, 0x00, 0x0C, 0x00, // DECODER_ID: 0x000C00D0
        ];
        assert_eq!(result, expected);
    }
}
