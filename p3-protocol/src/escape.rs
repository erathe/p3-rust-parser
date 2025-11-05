use crate::error::EscapeError;
/// Escape Sequence Encoder for P3 Protocol
///
/// Control bytes (0x8A through 0x8F) appearing in message data must be escaped
/// to avoid confusion with frame markers (SOR=0x8E, EOR=0x8F, ESCAPE=0x8D).
///
/// Escape mechanism:
/// - Prefix the byte with 0x8D (ESCAPE)
/// - Add 0x20 to the original byte value
///
/// Example: 0x8F → [0x8D, 0xAF]  (0x8F + 0x20 = 0xAF)
///
/// The LENGTH field in the message header does NOT include escape bytes.
/// It reflects the unescaped message length.
use crate::types::{ESCAPE, ESCAPE_OFFSET, ESCAPE_RANGE_END, ESCAPE_RANGE_START};

/// Information about escape encoding
#[derive(Debug, Clone, PartialEq)]
pub struct EscapeInfo {
    /// Original (unescaped) length
    pub unescaped_length: usize,
    /// Length after escaping
    pub escaped_length: usize,
    /// Number of bytes that needed escaping
    pub escape_count: usize,
}

/// Checks if a byte needs to be escaped
///
/// Bytes in range 0x8A-0x8F (inclusive) must be escaped.
#[inline]
pub fn needs_escape(byte: u8) -> bool {
    byte >= ESCAPE_RANGE_START && byte <= ESCAPE_RANGE_END
}

/// Escapes a single byte
///
/// Returns the escaped sequence [ESCAPE, byte + ESCAPE_OFFSET]
#[inline]
fn escape_byte(byte: u8) -> [u8; 2] {
    [ESCAPE, byte.wrapping_add(ESCAPE_OFFSET)]
}

/// Encodes data by applying escape sequences
///
/// Scans the input data and escapes any control bytes (0x8A-0x8F).
/// Returns a new Vec containing the escaped data.
///
/// # Example
/// ```
/// use p3_test_server::generator::escape::encode;
///
/// // Original data containing control byte 0x8F
/// let data = vec![0x01, 0x8F, 0x02];
///
/// // Escaped data: 0x8F becomes [0x8D, 0xAF]
/// let escaped = encode(&data);
/// assert_eq!(escaped, vec![0x01, 0x8D, 0xAF, 0x02]);
/// ```
pub fn encode(data: &[u8]) -> Vec<u8> {
    let mut escaped = Vec::with_capacity(escaped_length(data));

    for &byte in data {
        if needs_escape(byte) {
            let [esc, val] = escape_byte(byte);
            escaped.push(esc);
            escaped.push(val);
        } else {
            escaped.push(byte);
        }
    }

    escaped
}

/// Calculates the unescaped length of data
///
/// This is the length value that should be used in the P3 message LENGTH field.
/// It equals the original data length (before escaping).
///
/// # Example
/// ```
/// use p3_test_server::generator::escape::unescaped_length;
///
/// let data = vec![0x01, 0x8F, 0x02];  // 3 bytes
/// assert_eq!(unescaped_length(&data), 3);
/// ```
#[inline]
pub fn unescaped_length(data: &[u8]) -> usize {
    data.len()
}

/// Calculates how many bytes the data will be after escaping
///
/// Useful for pre-allocating buffers when you know the data will be escaped.
pub fn escaped_length(data: &[u8]) -> usize {
    data.iter()
        .fold(0, |acc, &b| acc + if needs_escape(b) { 2 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_needs_escape() {
        // Control bytes that need escaping
        assert!(needs_escape(0x8A));
        assert!(needs_escape(0x8B));
        assert!(needs_escape(0x8C));
        assert!(needs_escape(0x8D));
        assert!(needs_escape(0x8E));
        assert!(needs_escape(0x8F));

        // Normal bytes that don't need escaping
        assert!(!needs_escape(0x00));
        assert!(!needs_escape(0x01));
        assert!(!needs_escape(0x89));
        assert!(!needs_escape(0x90));
        assert!(!needs_escape(0xFF));
    }

    #[test]
    fn test_escape_byte() {
        assert_eq!(escape_byte(0x8A), [0x8D, 0xAA]);
        assert_eq!(escape_byte(0x8B), [0x8D, 0xAB]);
        assert_eq!(escape_byte(0x8C), [0x8D, 0xAC]);
        assert_eq!(escape_byte(0x8D), [0x8D, 0xAD]);
        assert_eq!(escape_byte(0x8E), [0x8D, 0xAE]);
        assert_eq!(escape_byte(0x8F), [0x8D, 0xAF]);
    }

    #[test]
    fn test_encode_no_escapes() {
        let data = vec![0x01, 0x02, 0x03];
        let escaped = encode(&data);
        assert_eq!(escaped, vec![0x01, 0x02, 0x03]);
    }

    #[test]
    fn test_encode_single_escape() {
        // Data with 0x8F that needs escaping
        let data = vec![0x01, 0x8F, 0x02];
        let escaped = encode(&data);
        assert_eq!(escaped, vec![0x01, 0x8D, 0xAF, 0x02]);
    }

    #[test]
    fn test_encode_multiple_escapes() {
        // Data with multiple control bytes
        let data = vec![0x8E, 0x8F, 0x8D];
        let escaped = encode(&data);
        assert_eq!(escaped, vec![0x8D, 0xAE, 0x8D, 0xAF, 0x8D, 0xAD]);
    }

    #[test]
    fn test_encode_all_control_bytes() {
        // Test all control bytes in the escape range
        let data = vec![0x8A, 0x8B, 0x8C, 0x8D, 0x8E, 0x8F];
        let escaped = encode(&data);
        assert_eq!(
            escaped,
            vec![
                0x8D, 0xAA, 0x8D, 0xAB, 0x8D, 0xAC, 0x8D, 0xAD, 0x8D, 0xAE, 0x8D, 0xAF
            ]
        );
    }

    #[test]
    fn test_encode_mixed_data() {
        // Realistic data with some control bytes
        let data = vec![0x00, 0x01, 0x8F, 0x03, 0x04, 0x8E, 0x05];
        let escaped = encode(&data);
        assert_eq!(
            escaped,
            vec![0x00, 0x01, 0x8D, 0xAF, 0x03, 0x04, 0x8D, 0xAE, 0x05]
        );
    }

    #[test]
    fn test_unescaped_length() {
        let data = vec![0x01, 0x8F, 0x02];
        assert_eq!(unescaped_length(&data), 3);

        let data = vec![0x8A, 0x8B, 0x8C];
        assert_eq!(unescaped_length(&data), 3);
    }

    #[test]
    fn test_escaped_length() {
        // No escapes needed
        let data = vec![0x01, 0x02, 0x03];
        assert_eq!(escaped_length(&data), 3);

        // One escape needed: [0x01, 0x8F, 0x02] → [0x01, 0x8D, 0xAF, 0x02]
        let data = vec![0x01, 0x8F, 0x02];
        assert_eq!(escaped_length(&data), 4);

        // Three escapes needed
        let data = vec![0x8A, 0x8B, 0x8C];
        assert_eq!(escaped_length(&data), 6);
    }

    #[test]
    fn test_encode_empty() {
        let data = vec![];
        let escaped = encode(&data);
        assert_eq!(escaped, vec![]);
    }

    /// Test based on real fixture data
    /// From passing_with_escapes.bin which contains sequence [0x8D, 0xAF]
    #[test]
    fn test_encode_real_fixture_scenario() {
        // If the original data had 0x8F, it would be escaped to [0x8D, 0xAF]
        let data = vec![0x8F];
        let escaped = encode(&data);
        assert_eq!(escaped, vec![0x8D, 0xAF]);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Property: encoding then decoding (via CRC unescape) should be identity
        #[test]
        fn test_encode_roundtrip(data in prop::collection::vec(any::<u8>(), 0..1000)) {
            let escaped = encode(&data);

            // Verify escaped length calculation is correct
            prop_assert_eq!(escaped.len(), escaped_length(&data));

            // Verify unescaped length is preserved
            prop_assert_eq!(unescaped_length(&data), data.len());
        }

        /// Property: escaped data should never contain unescaped control bytes
        #[test]
        fn test_no_unescaped_control_bytes(data in prop::collection::vec(any::<u8>(), 0..100)) {
            let escaped = encode(&data);

            // After escaping, no control bytes (0x8A-0x8F) should appear alone
            // They should only appear after an ESCAPE byte
            let mut i = 0;
            while i < escaped.len() {
                if escaped[i] == ESCAPE && i + 1 < escaped.len() {
                    // This is an escape sequence, skip both bytes
                    i += 2;
                } else {
                    // This byte should NOT be a control byte
                    prop_assert!(!needs_escape(escaped[i]),
                        "Found unescaped control byte 0x{:02X} at position {}", escaped[i], i);
                    i += 1;
                }
            }
        }

        /// Property: all bytes in escape range should be doubled in length
        #[test]
        fn test_escape_range_bytes_doubled(byte in 0x8Au8..=0x8F) {
            let data = vec![byte];
            let escaped = encode(&data);
            prop_assert_eq!(escaped.len(), 2);
            prop_assert_eq!(escaped[0], ESCAPE);
            prop_assert_eq!(escaped[1], byte + ESCAPE_OFFSET);
        }

        /// Property: bytes outside escape range should pass through unchanged
        #[test]
        fn test_non_escape_bytes_unchanged(data in prop::collection::vec(0u8..0x8A, 0..100)) {
            let escaped = encode(&data);
            prop_assert_eq!(escaped, data);
        }

        /// Property: escaped_length should always be >= original length
        #[test]
        fn test_escaped_length_monotonic(data in prop::collection::vec(any::<u8>(), 0..100)) {
            let escaped_len = escaped_length(&data);
            prop_assert!(escaped_len >= data.len(),
                "Escaped length {} should be >= original length {}", escaped_len, data.len());
        }

        /// Property: multiple consecutive control bytes should each be escaped
        #[test]
        fn test_consecutive_control_bytes(count in 1usize..20) {
            let data = vec![0x8F; count];
            let escaped = encode(&data);
            prop_assert_eq!(escaped.len(), count * 2);

            // Verify all are properly escaped
            for i in 0..count {
                prop_assert_eq!(escaped[i * 2], ESCAPE);
                prop_assert_eq!(escaped[i * 2 + 1], 0xAF);
            }
        }
    }
}

// Public API aliases for better naming
/// Alias for `encode` - escapes control bytes in data
pub fn escape_data(data: &[u8]) -> Vec<u8> {
    encode(data)
}

/// Decodes data by removing escape sequences
///
/// Scans the input data and removes escape sequences, converting
/// [0x8D, 0xAX-0xAF] back to [0x8A-0x8F].
///
/// Returns an error if an incomplete or invalid escape sequence is found.
///
/// # Example
/// ```
/// use p3_protocol::escape::unescape_data;
///
/// // Escaped data: [0x8D, 0xAF] represents 0x8F
/// let escaped = vec![0x01, 0x8D, 0xAF, 0x02];
///
/// let unescaped = unescape_data(&escaped).unwrap();
/// assert_eq!(unescaped, vec![0x01, 0x8F, 0x02]);
/// ```
pub fn unescape_data(data: &[u8]) -> Result<Vec<u8>, EscapeError> {
    let mut unescaped = Vec::with_capacity(data.len());
    let mut i = 0;

    while i < data.len() {
        if data[i] == ESCAPE {
            // Check if there's a following byte
            if i + 1 >= data.len() {
                return Err(EscapeError::IncompleteSequence);
            }

            let next_byte = data[i + 1];

            // Validate that the escaped byte is in valid range (0xAA-0xAF)
            if next_byte < 0xAA || next_byte > 0xAF {
                return Err(EscapeError::InvalidSequence(next_byte));
            }

            // Unescape: subtract offset to get original byte
            let original_byte = next_byte.wrapping_sub(ESCAPE_OFFSET);
            unescaped.push(original_byte);
            i += 2; // Skip both escape and escaped byte
        } else {
            unescaped.push(data[i]);
            i += 1;
        }
    }

    Ok(unescaped)
}
