use crate::error::{ParseError, ParseResult};

#[derive(Debug, Clone, PartialEq)]
pub struct TlvField {
    pub tag: u8,
    pub value: Vec<u8>,
}

pub struct TlvDecoder {
    // TODO: Add state management if needed
}

impl TlvDecoder {
    pub fn new() -> Self {
        Self {}
    }

    /// Decode all TLV fields from data
    ///
    /// Parses Tag-Length-Value fields from the message body.
    /// Each field: [Tag: 1 byte][Length: 1 byte][Value: Length bytes]
    pub fn decode(&self, data: &[u8]) -> ParseResult<Vec<TlvField>> {
        let mut fields = Vec::new();
        let mut pos = 0;

        while pos < data.len() {
            // Need at least 2 bytes for tag and length
            if pos + 2 > data.len() {
                return Err(ParseError::TlvError(format!(
                    "Incomplete TLV field at position {}",
                    pos
                )));
            }

            let tag = data[pos];
            let length = data[pos + 1] as usize;
            pos += 2;

            // Check if we have enough bytes for the value
            if pos + length > data.len() {
                return Err(ParseError::TlvError(format!(
                    "Incomplete TLV value for tag 0x{:02X}: expected {} bytes, got {}",
                    tag,
                    length,
                    data.len() - pos
                )));
            }

            // Extract value bytes
            let value = data[pos..pos + length].to_vec();
            pos += length;

            fields.push(TlvField { tag, value });
        }

        Ok(fields)
    }

    pub fn decode_u32(bytes: &[u8]) -> Option<u32> {
        bytes.try_into().ok().map(u32::from_le_bytes)
    }

    pub fn decode_u16(bytes: &[u8]) -> Option<u16> {
        bytes.try_into().ok().map(u16::from_le_bytes)
    }

    pub fn decode_u64(bytes: &[u8]) -> Option<u64> {
        bytes.try_into().ok().map(u64::from_le_bytes)
    }
}

impl Default for TlvDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_u32() {
        let bytes = vec![0x12, 0x34, 0x56, 0x78];
        assert_eq!(TlvDecoder::decode_u32(&bytes), Some(0x78563412));
    }

    #[test]
    fn test_decode_u16() {
        let bytes = vec![0x12, 0x34];
        assert_eq!(TlvDecoder::decode_u16(&bytes), Some(0x3412));
    }

    #[test]
    #[ignore]
    fn test_decode_tlv() {
        // TODO: Add tests
    }
}
