//! Regression tests: the parser must reject frames whose CRC does not match.
//!
//! The parser previously called `validate_crc` but discarded its boolean
//! result, accepting corrupted messages. Messages here are live captures from
//! decoder D0000C00 (2025-10-30).

use p3_parser::{Message, ParseError, Parser};
use p3_protocol::CrcError;

/// Live-captured STATUS message, CRC = 0xC318
const LIVE_STATUS: [u8; 31] = [
    0x8E, 0x02, 0x1F, 0x00, 0x18, 0xC3, // CRC: 0xC318
    0x00, 0x00, 0x02, 0x00, 0x01, 0x02, 0x3B, 0x00, 0x07, 0x02, 0x0A, 0x00, 0x06, 0x01,
    0x01, 0x0A, 0x01, 0x00, 0x81, 0x04, 0xD0, 0x00, 0x0C, 0x00, 0x8F,
];

#[test]
fn valid_live_message_parses() {
    let parser = Parser::new();
    match parser.parse(&LIVE_STATUS) {
        Ok(Message::Status(status)) => {
            assert_eq!(status.noise, 59);
            assert_eq!(status.gps_status, 1);
            assert_eq!(status.temperature, 10);
            assert_eq!(status.satellites, 0);
            assert_eq!(status.decoder_id.as_deref(), Some("D0000C00"));
        }
        other => panic!("expected STATUS, got {:?}", other),
    }
}

#[test]
fn corrupted_crc_field_is_rejected() {
    let mut message = LIVE_STATUS;
    message[4] = 0xFF;
    message[5] = 0xFF;

    let parser = Parser::new();
    match parser.parse(&message) {
        Err(ParseError::CrcError(CrcError::ValidationFailed { expected, actual })) => {
            assert_eq!(expected, 0xC318);
            assert_eq!(actual, 0xFFFF);
        }
        other => panic!("expected CRC ValidationFailed, got {:?}", other.map(|_| ())),
    }
}

#[test]
fn corrupted_body_is_rejected() {
    let mut message = LIVE_STATUS;
    message[12] = 0x3C; // NOISE value 0x3B -> 0x3C, CRC field left stale

    let parser = Parser::new();
    match parser.parse(&message) {
        Err(ParseError::CrcError(CrcError::ValidationFailed { .. })) => {}
        other => panic!("expected CRC ValidationFailed, got {:?}", other.map(|_| ())),
    }
}
