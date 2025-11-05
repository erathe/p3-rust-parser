use crate::error::CrcError;
/// CRC-16 Implementation for P3 Protocol
///
/// **Source:** HobbyTalk forum post #65 by aminear (November 21, 2015)
///
/// ## Algorithm Details
/// - **Polynomial:** 0x1021 (CRC-16-CCITT)
/// - **Initial value:** 0xFFFF
/// - **Lookup table:** Pre-computed 256-entry table for efficiency
/// - **No reflection:** Input/output bits are not reversed
/// - **No final XOR:** Result is used directly
///
/// ## P3 Protocol CRC Calculation Process
/// 1. Remove all escape sequences (0x8D prefix bytes) from the message
/// 2. Set CRC field bytes (positions 4-5) to 0x00
/// 3. Calculate CRC-16 over the entire message (including SOR 0x8E and EOR 0x8F)
/// 4. Store result as 16-bit little-endian value at positions 4-5
use crate::types::{ESCAPE, ESCAPE_OFFSET, OFFSET_CRC, SIZE_U16_FIELD};

/// CRC-16 lookup table (polynomial 0x1021)
///
/// Pre-computed at compile time.
static CRC16_TABLE: [u16; 256] = init_crc16_table();

const fn init_crc16_table() -> [u16; 256] {
    let mut table = [0u16; 256];
    let mut i = 0;

    while i < 256 {
        let mut crc = (i as u16) << 8;
        let mut j = 0;

        while j < 8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc = crc << 1;
            }
            j += 1;
        }

        table[i] = crc;
        i += 1;
    }

    table
}

/// Calculate CRC-16 for data
///
/// # Arguments
/// * `data` - Message data with CRC field already zeroed
///
/// # Returns
/// 16-bit CRC value
///
/// # Example
/// ```
/// use p3_protocol::calculate_crc;
///
/// // Message with CRC field zeroed
/// let message = vec![0x8E, 0x02, 0x0B, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x8F];
/// let crc = calculate_crc(&message);
/// ```
pub fn calculate_crc(data: &[u8]) -> u16 {
    let mut crc = 0xFFFFu16;

    for &byte in data {
        let index = ((crc >> 8) & 0xFF) as u8;
        crc = CRC16_TABLE[index as usize] ^ (crc << 8) ^ (byte as u16);
    }

    crc
}

/// Calculate CRC for a complete P3 message
///
/// Handles the complete process:
/// 1. Unescapes the message
/// 2. Zeros out the CRC field
/// 3. Calculates and returns the CRC
///
/// # Arguments
/// * `escaped_message` - Complete message including escapes, SOR, and EOR
///
/// # Errors
/// Returns `CrcError::MalformedEscape` if the message contains invalid escape sequences.
/// Returns `CrcError::MessageTooShort` if the message is too short to contain a CRC field.
///
/// # Returns
/// 16-bit CRC value in host byte order (use `to_le_bytes()` for message)
pub fn calculate_message_crc(escaped_message: &[u8]) -> Result<u16, CrcError> {
    let unescaped = unescape_message(escaped_message)?;

    let min_size = OFFSET_CRC + SIZE_U16_FIELD;
    if unescaped.len() < min_size {
        return Err(CrcError::MessageTooShort {
            actual: unescaped.len(),
            min: min_size,
        });
    }

    // Zero out CRC field
    let mut message_for_crc = unescaped;
    message_for_crc[OFFSET_CRC] = 0x00;
    message_for_crc[OFFSET_CRC + 1] = 0x00;

    Ok(calculate_crc(&message_for_crc))
}

/// Validate CRC for a complete P3 message
///
/// # Arguments
/// * `escaped_message` - Complete message including escapes, SOR, and EOR
///
/// # Errors
/// Returns `CrcError::MalformedEscape` if the message contains invalid escape sequences.
/// Returns `CrcError::MessageTooShort` if the message is too short to contain a CRC field.
///
/// # Returns
/// `true` if CRC is valid, `false` otherwise
///
/// # Example
/// ```
/// use p3_protocol::validate_crc;
///
/// // Complete status message with valid CRC
/// let message = vec![
///     0x8E, 0x02, 0x1F, 0x00,
///     0x3D, 0x27, // CRC: 0x273D
///     0x00, 0x00, 0x02, 0x00,
///     0x01, 0x02, 0x1B, 0x00,
///     0x07, 0x02, 0x21, 0x00,
///     0x0C, 0x01, 0x7A,
///     0x06, 0x01, 0x00,
///     0x81, 0x04, 0xFC, 0x05, 0x04, 0x00,
///     0x8F,
/// ];
/// assert!(validate_crc(&message).unwrap());
/// ```
pub fn validate_crc(escaped_message: &[u8]) -> Result<bool, CrcError> {
    let unescaped = unescape_message(escaped_message)?;

    let min_size = OFFSET_CRC + SIZE_U16_FIELD;
    if unescaped.len() < min_size {
        return Err(CrcError::MessageTooShort {
            actual: unescaped.len(),
            min: min_size,
        });
    }

    // Extract CRC from message (little-endian)
    let message_crc = u16::from_le_bytes([unescaped[OFFSET_CRC], unescaped[OFFSET_CRC + 1]]);

    // Calculate expected CRC
    let calculated_crc = calculate_message_crc(escaped_message)?;

    Ok(message_crc == calculated_crc)
}

/// Remove escape sequences from a message
///
/// Escape sequences in P3 protocol:
/// - `0x8D` followed by `(byte + 0x20)` represents the original `byte`
/// - Example: `0x8D 0xAF` represents `0x8F`
///
/// # Errors
/// Returns `CrcError::MalformedEscape` if an escape byte (0x8D) is followed by
/// a byte outside the valid range (0xAA-0xAF).
fn unescape_message(escaped: &[u8]) -> Result<Vec<u8>, CrcError> {
    let mut unescaped = Vec::with_capacity(escaped.len());
    let mut i = 0;

    while i < escaped.len() {
        if escaped[i] == ESCAPE {
            if i + 1 >= escaped.len() {
                // Escape at end of message - treat as literal
                unescaped.push(escaped[i]);
                i += 1;
                continue;
            }

            let next_byte = escaped[i + 1];
            // Valid escape: next byte should be in range 0xAA-0xAF (0x8A-0x8F + 0x20)
            if next_byte >= 0xAA && next_byte <= 0xAF {
                unescaped.push(next_byte.wrapping_sub(ESCAPE_OFFSET));
                i += 2;
                continue;
            } else {
                return Err(CrcError::MalformedEscape {
                    position: i,
                    next_byte,
                });
            }
        }
        unescaped.push(escaped[i]);
        i += 1;
    }

    Ok(unescaped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc_table() {
        assert_eq!(CRC16_TABLE[0], 0x0000);
        assert_eq!(CRC16_TABLE[1], 0x1021);
        assert_eq!(CRC16_TABLE[255], 0x1EF0);
    }

    #[test]
    fn test_unescape() {
        // Test basic escape sequence
        let escaped = vec![0x01, 0x8D, 0xAF, 0x02];
        let unescaped = unescape_message(&escaped).unwrap();
        assert_eq!(unescaped, vec![0x01, 0x8F, 0x02]);

        // Test multiple escapes
        let escaped = vec![0x8D, 0xAA, 0x8D, 0xAB, 0x8D, 0xAF];
        let unescaped = unescape_message(&escaped).unwrap();
        assert_eq!(unescaped, vec![0x8A, 0x8B, 0x8F]);

        // Test no escapes
        let escaped = vec![0x01, 0x02, 0x03];
        let unescaped = unescape_message(&escaped).unwrap();
        assert_eq!(unescaped, vec![0x01, 0x02, 0x03]);
    }

    #[test]
    fn test_unescape_malformed() {
        // Test malformed escape: 0x8D followed by invalid byte
        let escaped = vec![0x01, 0x8D, 0x50, 0x02];
        let result = unescape_message(&escaped);
        assert!(matches!(
            result,
            Err(CrcError::MalformedEscape {
                position: 1,
                next_byte: 0x50
            })
        ));
    }

    #[test]
    fn test_status_message() {
        let message = vec![
            0x8E, 0x02, 0x1F, 0x00, 0x3D, 0x27, // CRC: 0x273D
            0x00, 0x00, 0x02, 0x00, 0x01, 0x02, 0x1B, 0x00, 0x07, 0x02, 0x21, 0x00, 0x0C, 0x01,
            0x7A, 0x06, 0x01, 0x00, 0x81, 0x04, 0xFC, 0x05, 0x04, 0x00, 0x8F,
        ];

        let calculated = calculate_message_crc(&message).unwrap();
        assert_eq!(calculated, 0x273D, "CRC mismatch for status message");
        assert!(
            validate_crc(&message).unwrap(),
            "Status message should validate"
        );
    }

    #[test]
    fn test_passing_message() {
        let message = vec![
            0x8E, 0x02, 0x33, 0x00, 0xCF, 0x02, // CRC: 0x02CF
            0x00, 0x00, 0x01, 0x00, 0x01, 0x04, 0xB2, 0x9B, 0x01, 0x00, 0x03, 0x04, 0x27, 0xFC,
            0x70, 0x00, 0x04, 0x08, 0xE8, 0x19, 0xE6, 0xBD, 0x8A, 0x75, 0x04, 0x00, 0x05, 0x02,
            0x33, 0x00, 0x06, 0x02, 0x10, 0x00, 0x08, 0x02, 0x00, 0x00, 0x81, 0x04, 0xFC, 0x05,
            0x04, 0x00, 0x8F,
        ];

        let calculated = calculate_message_crc(&message).unwrap();
        assert_eq!(calculated, 0x02CF, "CRC mismatch for passing message");
        assert!(
            validate_crc(&message).unwrap(),
            "Passing message should validate"
        );
    }

    #[test]
    fn test_crc_with_invalid_data() {
        // Message too short
        let short_message = vec![0x8E, 0x02, 0x00];
        assert!(validate_crc(&short_message).is_err());

        // Message with wrong CRC (should return Ok(false))
        let bad_crc = vec![
            0x8E, 0x02, 0x1F, 0x00, 0xFF, 0xFF, // Wrong CRC
            0x00, 0x00, 0x02, 0x00, 0x8F,
        ];
        assert_eq!(validate_crc(&bad_crc).unwrap(), false);
    }

    #[test]
    fn test_crc_bytes_conversion() {
        // Test that we can correctly convert CRC to/from little-endian bytes
        let crc = 0x273Du16;
        let bytes = crc.to_le_bytes();
        assert_eq!(bytes, [0x3D, 0x27]);

        let crc2 = u16::from_le_bytes([0x3D, 0x27]);
        assert_eq!(crc2, 0x273D);
    }

    #[test]
    fn test_forum_message_with_escape() {
        // Real message from HobbyTalk forum post
        // Contains escape sequence 8d af (unescapes to 8f) in field 04
        let message = vec![
            0x8E, 0x02, 0x33, 0x00, 0xEB, 0x1D, // CRC: 0x1DEB
            0x00, 0x00, 0x01, 0x00, 0x01, 0x04, 0x9D, 0x09, 0x00, 0x00, 0x03, 0x04, 0xE4, 0xD2,
            0x36, 0x00, 0x04, 0x08, 0x10, 0x79, 0x8D, 0xAF, 0xE4, 0xF2, 0xCE, 0x04, 0x00, 0x05,
            0x02, 0x5F, 0x00, 0x06, 0x02, 0x2E, 0x00, 0x08, 0x02, 0x00, 0x00, 0x81, 0x04, 0xBE,
            0x13, 0x04, 0x00, 0x8F,
        ];

        // First, verify the unescape works correctly
        let unescaped = unescape_message(&message).unwrap();
        // After unescaping, 8d af should become 8f, making the message 1 byte shorter
        assert_eq!(
            unescaped.len(),
            message.len() - 1,
            "Message should be 1 byte shorter after unescaping"
        );

        // Calculate CRC
        let calculated = calculate_message_crc(&message).unwrap();

        // Debug output if CRC doesn't match
        if calculated != 0x1DEB {
            eprintln!("CRC mismatch!");
            eprintln!("Expected: 0x{:04X}", 0x1DEB);
            eprintln!("Calculated: 0x{:04X}", calculated);
            eprintln!("Unescaped length: {}", unescaped.len());
        }

        assert_eq!(
            calculated, 0x1DEB,
            "CRC mismatch for forum message with escape"
        );
        assert!(
            validate_crc(&message).unwrap(),
            "Forum message should validate"
        );
    }

    #[test]
    fn test_passing_with_escapes_fixture() {
        // Test fixture file: passing_with_escapes.bin
        // Similar to forum message but with different field values (05, 06)
        // Contains escape sequence 8d af in field 04
        let message = vec![
            0x8E, 0x02, 0x33, 0x00, 0x83, 0xF5, // CRC: 0xF583
            0x00, 0x00, 0x01, 0x00, 0x01, 0x04, 0x9D, 0x09, 0x00, 0x00, 0x03, 0x04, 0xE4, 0xD2,
            0x36, 0x00, 0x04, 0x08, 0x10, 0x79, 0x8D, 0xAF, 0xE4, 0xF2, 0xCE, 0x04, 0x00, 0x05,
            0x02, 0x72, 0x00, 0x06, 0x02, 0x27, 0x00, 0x08, 0x02, 0x00, 0x00, 0x8F,
        ];

        let calculated = calculate_message_crc(&message).unwrap();
        assert_eq!(
            calculated, 0xF583,
            "CRC mismatch for passing_with_escapes fixture"
        );
        assert!(
            validate_crc(&message).unwrap(),
            "Fixture message should validate"
        );
    }

    #[test]
    fn test_all_escapes_fixture() {
        // Test fixture file: all_escapes.bin
        // Edge case: Contains ALL escapable bytes (0x8A through 0x8E)
        // Tests comprehensive escape sequence handling
        let message = vec![
            0x8E, 0x02, 0x25, 0x00, 0x57, 0xE9, // CRC: 0xE957
            0x00, 0x00, 0x02, 0x00, 0x01, 0x02, 0x8D, 0xAA, 0x00, // 8d aa -> 8a
            0x07, 0x02, 0x8D, 0xAB, 0x00, // 8d ab -> 8b
            0x0C, 0x01, 0x8D, 0xAC, // 8d ac -> 8c
            0x06, 0x01, 0x8D, 0xAD, // 8d ad -> 8d
            0x81, 0x04, 0x8D, 0xAE, 0x05, 0x04, 0x00, // 8d ae -> 8e
            0x8F,
        ];

        // Verify unescaping reduces message length by 5 bytes (5 escape sequences)
        let unescaped = unescape_message(&message).unwrap();
        assert_eq!(
            unescaped.len(),
            message.len() - 5,
            "Message should be 5 bytes shorter after unescaping 5 sequences"
        );

        let calculated = calculate_message_crc(&message).unwrap();
        assert_eq!(calculated, 0xE957, "CRC mismatch for all_escapes fixture");
        assert!(
            validate_crc(&message).unwrap(),
            "All escapes fixture should validate"
        );
    }

    #[test]
    fn test_live_captured_status_message() {
        // Real STATUS message captured from MyLaps ProChip Smart Decoder
        // Contains actual field data:
        // - NOISE: 59 (0x3B)
        // - TEMPERATURE: 10 → 1.0°C (tag 0x07, value 0x0A)
        // - GPS: 1
        // - SATINUSE: 0 (tag 0x0A)
        // - DECODER_ID: 0x000C00D0 (tag 0x81)
        let message = vec![
            0x8E, 0x02, 0x1F, 0x00, 0x18, 0xC3, // CRC: 0xC318
            0x00, 0x00, 0x02, 0x00, 0x01, 0x02, 0x3B, 0x00, 0x07, 0x02, 0x0A, 0x00, 0x06, 0x01,
            0x01, 0x0A, 0x01, 0x00, 0x81, 0x04, 0xD0, 0x00, 0x0C, 0x00, 0x8F,
        ];

        let calculated = calculate_message_crc(&message).unwrap();

        // Debug output if test fails
        if calculated != 0xC318 {
            eprintln!("Live capture CRC validation failed!");
            eprintln!("Expected CRC: 0x{:04X}", 0xC318);
            eprintln!("Calculated CRC: 0x{:04X}", calculated);
            eprintln!(
                "Message hex: {}",
                message
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<String>()
            );
        }

        assert_eq!(
            calculated, 0xC318,
            "CRC mismatch for live captured STATUS message"
        );
        assert!(
            validate_crc(&message).unwrap(),
            "Live captured message should validate"
        );
    }

    #[test]
    fn test_live_captured_passing_with_transponder_string() {
        // Real PASSING message captured from MyLaps ProChip Smart Decoder
        // This is a rider transponder with Tag 0x0A string field "FL-94890"
        // Contains actual timing data:
        // - PASSING_NUMBER: 8,857 (0x2299)
        // - TRANSPONDER: 0x061FF72A = 102,758,186
        // - STRING (Tag 0x0A): "FL-94890" (rider identifier)
        // - STRENGTH: 133 (0x85)
        // - HITS: 29 (0x1D)
        // - RTC_TIME:
        let message = vec![
            0x8E, 0x02, 0x3D, 0x00, 0x12, 0x85, // CRC: 0x8512
            0x00, 0x00, 0x01, 0x00, 0x01, 0x04, 0x99, 0x22, 0x00, 0x00, 0x03, 0x04, 0x2A, 0xF7,
            0x1F, 0x06, 0x0A, 0x08, 0x46, 0x4C, 0x2D, 0x39, 0x34, 0x38, 0x39,
            0x30, // "FL-94890"
            0x05, 0x02, 0x85, 0x00, 0x06, 0x02, 0x1D, 0x00, 0x04, 0x08, 0x85, 0x01, 0xCA, 0x08,
            0x66, 0x42, 0x06, 0x00, 0x08, 0x02, 0x00, 0x00, 0x81, 0x04, 0xD0, 0x00, 0x0C, 0x00,
            0x8F,
        ];

        let calculated = calculate_message_crc(&message).unwrap();

        // Debug output if test fails
        if calculated != 0x8512 {
            eprintln!("Live PASSING CRC validation failed!");
            eprintln!("Expected CRC: 0x{:04X}", 0x8512);
            eprintln!("Calculated CRC: 0x{:04X}", calculated);
            eprintln!(
                "Message hex: {}",
                message
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<String>()
            );
        }

        assert_eq!(
            calculated, 0x8512,
            "CRC mismatch for live PASSING message with transponder string"
        );
        assert!(
            validate_crc(&message).unwrap(),
            "Live PASSING message should validate"
        );
    }

    #[test]
    fn test_live_captured_passing_start_gate() {
        // Real PASSING message from start gate pulse
        // This is a start gate transponder (ID 9,995) - marks when gate drops
        // Contains timing data:
        // - PASSING_NUMBER: 8,859 (0x229B)
        // - TRANSPONDER: 0x0000270B = 9,995 (start gate ID)
        // - NO STRING FIELD (Tag 0x0A) - that's why it's only 43 bytes
        // - RTC_TIME: Gate drop timestamp
        let message = vec![
            0x8E, 0x02, 0x2B, 0x00, 0x22, 0x91, // CRC: 0x9122
            0x00, 0x00, 0x01, 0x00, 0x01, 0x04, 0x9B, 0x22, 0x00, 0x00, 0x03, 0x04, 0x0B, 0x27,
            0x00, 0x00, 0x04, 0x08, 0xE8, 0x34, 0xCF, 0x0A, 0x66, 0x42, 0x06, 0x00, 0x08, 0x02,
            0x00, 0x00, 0x81, 0x04, 0xD0, 0x00, 0x0C, 0x00, 0x8F,
        ];

        let calculated = calculate_message_crc(&message).unwrap();

        // Debug output if test fails
        if calculated != 0x9122 {
            eprintln!("Live start gate CRC validation failed!");
            eprintln!("Expected CRC: 0x{:04X}", 0x9122);
            eprintln!("Calculated CRC: 0x{:04X}", calculated);
            eprintln!(
                "Message hex: {}",
                message
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<String>()
            );
        }

        assert_eq!(
            calculated, 0x9122,
            "CRC mismatch for live start gate PASSING message"
        );
        assert!(
            validate_crc(&message).unwrap(),
            "Live start gate message should validate"
        );
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Property: CRC of the same data should always be the same
        #[test]
        fn test_crc_deterministic(data in prop::collection::vec(any::<u8>(), 10..100)) {
            let crc1 = calculate_crc(&data);
            let crc2 = calculate_crc(&data);
            prop_assert_eq!(crc1, crc2);
        }

        /// Property: CRC should change if any byte changes
        #[test]
        fn test_crc_changes_on_modification(
            data in prop::collection::vec(any::<u8>(), 10..100),
            pos in 0usize..100,
            new_byte in any::<u8>()
        ) {
            if pos >= data.len() {
                return Ok(());
            }

            let original_crc = calculate_crc(&data);

            let mut modified = data.clone();
            if modified[pos] != new_byte {
                modified[pos] = new_byte;
                let modified_crc = calculate_crc(&modified);
                prop_assert_ne!(original_crc, modified_crc,
                    "CRC should change when byte at position {} changes", pos);
            }
        }

        /// Property: Unescaping valid escape sequences should not error
        #[test]
        fn test_unescape_valid_sequences(
            data in prop::collection::vec(0xAAu8..=0xAF, 1..50)
        ) {
            // Build a message with valid escape sequences
            let mut escaped = Vec::new();
            for &byte in &data {
                escaped.push(ESCAPE);
                escaped.push(byte);
            }

            let result = unescape_message(&escaped);
            prop_assert!(result.is_ok());

            let unescaped = result.unwrap();
            prop_assert_eq!(unescaped.len(), data.len());

            // Verify each byte was properly unescaped
            for (i, &original) in data.iter().enumerate() {
                prop_assert_eq!(unescaped[i], original - ESCAPE_OFFSET);
            }
        }

        /// Property: Malformed escape sequences should be detected (low range)
        #[test]
        fn test_malformed_escape_detected_low(invalid_byte in 0u8..0xAA) {
            let escaped = vec![ESCAPE, invalid_byte];
            let result = unescape_message(&escaped);
            prop_assert!(result.is_err(), "Expected error for invalid byte 0x{:02X}", invalid_byte);
        }

        /// Property: Malformed escape sequences should be detected (high range)
        #[test]
        fn test_malformed_escape_detected_high(invalid_byte in 0xB0u8..=0xFF) {
            let escaped = vec![ESCAPE, invalid_byte];
            let result = unescape_message(&escaped);
            prop_assert!(result.is_err(), "Expected error for invalid byte 0x{:02X}", invalid_byte);
        }

        /// Property: Empty data should have a deterministic CRC
        #[test]
        fn test_empty_crc_deterministic(_n in 0..10) {
            let crc1 = calculate_crc(&[]);
            let crc2 = calculate_crc(&[]);
            prop_assert_eq!(crc1, crc2);
            prop_assert_eq!(crc1, 0xFFFF); // Initial value
        }
    }
}
