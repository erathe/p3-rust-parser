//! Stream parsing for P3 data arriving in arbitrary chunks (e.g. from TCP).
//!
//! A [`StreamParser`] buffers incoming bytes and extracts complete messages:
//!
//! ```rust,ignore
//! let mut stream = StreamParser::new();
//! loop {
//!     let chunk = read_from_socket();
//!     stream.feed(&chunk);
//!     while let Some(result) = stream.next_message() {
//!         match result {
//!             Ok(message) => handle(message),
//!             Err(e) => eprintln!("bad frame: {}", e),
//!         }
//!     }
//! }
//! ```
//!
//! # Framing
//!
//! Frames are delimited by scanning for raw SOR (0x8E) and EOR (0x8F) bytes.
//! Control bytes 0x8A-0x8F are always escaped inside frame data on the wire,
//! so a raw SOR/EOR is unambiguously a frame boundary. This is deliberately
//! independent of the LENGTH header field: LENGTH bytes in the range
//! 0x8A-0x8F (messages of 138-143 unescaped bytes) are themselves escaped on
//! the wire, so framing by reading LENGTH out of the raw buffer would break.
//!
//! Garbage before a frame and truncated frames (a SOR with no EOR before the
//! next SOR) are discarded silently.

use crate::Parser;
use crate::error::ParseResult;
use crate::messages::Message;
use p3_protocol::{EOR, SOR};

/// Maximum bytes buffered while waiting for a frame to complete.
///
/// LENGTH is a u16, so an unescaped frame is at most 65,535 bytes; fully
/// escaped that is ~131 KiB on the wire. A buffer beyond that without an EOR
/// means the stream is corrupt, and the buffered data is dropped.
const MAX_BUFFER: usize = 256 * 1024;

/// Incremental parser for a byte stream containing P3 messages
pub struct StreamParser {
    parser: Parser,
    buffer: Vec<u8>,
}

impl StreamParser {
    pub fn new() -> Self {
        Self {
            parser: Parser::new(),
            buffer: Vec::new(),
        }
    }

    /// Append bytes received from the transport
    pub fn feed(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    /// Extract and parse the next complete message from the buffer
    ///
    /// Returns `None` when no complete frame is buffered yet (feed more
    /// bytes). Returns `Some(Err(..))` for a complete but invalid frame
    /// (e.g. CRC mismatch); the frame has been discarded and the caller can
    /// keep calling for subsequent messages.
    pub fn next_message(&mut self) -> Option<ParseResult<Message>> {
        let frame_end = self.find_frame()?;
        let result = self.parser.parse(&self.buffer[..frame_end]);
        self.buffer.drain(..frame_end);
        Some(result)
    }

    /// Locate the next complete frame, discarding garbage and truncated frames
    ///
    /// On `Some(end)`, the buffer starts with SOR and `end` is one past the
    /// frame's EOR.
    fn find_frame(&mut self) -> Option<usize> {
        loop {
            // Discard anything before the first SOR
            match self.buffer.iter().position(|&b| b == SOR) {
                Some(0) => {}
                Some(pos) => {
                    self.buffer.drain(..pos);
                }
                None => {
                    self.buffer.clear();
                    return None;
                }
            }

            // Scan past the SOR for the frame terminator or a restarted frame
            match self.buffer[1..].iter().position(|&b| b == EOR || b == SOR) {
                Some(pos) if self.buffer[pos + 1] == EOR => return Some(pos + 2),
                Some(pos) => {
                    // Another SOR before any EOR: the first frame was truncated
                    self.buffer.drain(..pos + 1);
                }
                None => {
                    if self.buffer.len() > MAX_BUFFER {
                        self.buffer.clear();
                    }
                    return None;
                }
            }
        }
    }
}

impl Default for StreamParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ParseError;

    /// Live-captured STATUS message (decoder D0000C00, 2025-10-30)
    const LIVE_STATUS: [u8; 31] = [
        0x8E, 0x02, 0x1F, 0x00, 0x18, 0xC3, // CRC: 0xC318
        0x00, 0x00, 0x02, 0x00, 0x01, 0x02, 0x3B, 0x00, 0x07, 0x02, 0x0A, 0x00, 0x06, 0x01,
        0x01, 0x0A, 0x01, 0x00, 0x81, 0x04, 0xD0, 0x00, 0x0C, 0x00, 0x8F,
    ];

    /// Live-captured PASSING message from a start gate (transponder 9995)
    const LIVE_GATE_PASSING: [u8; 43] = [
        0x8E, 0x02, 0x2B, 0x00, 0x22, 0x91, // CRC: 0x9122
        0x00, 0x00, 0x01, 0x00, 0x01, 0x04, 0x9B, 0x22, 0x00, 0x00, 0x03, 0x04, 0x0B, 0x27,
        0x00, 0x00, 0x04, 0x08, 0xE8, 0x34, 0xCF, 0x0A, 0x66, 0x42, 0x06, 0x00, 0x08, 0x02,
        0x00, 0x00, 0x81, 0x04, 0xD0, 0x00, 0x0C, 0x00, 0x8F,
    ];

    fn expect_status(result: Option<ParseResult<Message>>) {
        match result {
            Some(Ok(Message::Status(status))) => {
                assert_eq!(status.noise, 59);
                assert_eq!(status.decoder_id.as_deref(), Some("D0000C00"));
            }
            other => panic!("expected STATUS message, got {:?}", other.map(|r| r.is_ok())),
        }
    }

    #[test]
    fn test_single_message_one_feed() {
        let mut stream = StreamParser::new();
        stream.feed(&LIVE_STATUS);
        expect_status(stream.next_message());
        assert!(stream.next_message().is_none());
    }

    #[test]
    fn test_message_fed_byte_by_byte() {
        let mut stream = StreamParser::new();
        for &byte in LIVE_STATUS.iter() {
            stream.feed(&[byte]);
        }
        expect_status(stream.next_message());
        assert!(stream.next_message().is_none());
    }

    #[test]
    fn test_incomplete_message_returns_none() {
        let mut stream = StreamParser::new();
        stream.feed(&LIVE_STATUS[..20]);
        assert!(stream.next_message().is_none());

        // Completing the frame makes it available
        stream.feed(&LIVE_STATUS[20..]);
        expect_status(stream.next_message());
    }

    #[test]
    fn test_multiple_messages_one_feed() {
        let mut stream = StreamParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(&LIVE_STATUS);
        data.extend_from_slice(&LIVE_GATE_PASSING);
        data.extend_from_slice(&LIVE_STATUS);
        stream.feed(&data);

        expect_status(stream.next_message());
        match stream.next_message() {
            Some(Ok(Message::Passing(passing))) => {
                assert_eq!(passing.transponder_id, 9995);
            }
            other => panic!("expected PASSING message, got {:?}", other.map(|r| r.is_ok())),
        }
        expect_status(stream.next_message());
        assert!(stream.next_message().is_none());
    }

    #[test]
    fn test_garbage_before_frame_is_skipped() {
        let mut stream = StreamParser::new();
        stream.feed(&[0x00, 0x42, 0x13, 0x37]);
        stream.feed(&LIVE_STATUS);
        expect_status(stream.next_message());
    }

    #[test]
    fn test_garbage_without_sor_is_dropped() {
        let mut stream = StreamParser::new();
        stream.feed(&[0x00, 0x42, 0x13, 0x37]);
        assert!(stream.next_message().is_none());
        assert!(stream.buffer.is_empty(), "garbage should not accumulate");
    }

    #[test]
    fn test_truncated_frame_then_complete_frame() {
        let mut stream = StreamParser::new();
        // A frame that starts but never ends (no EOR), then a complete one
        stream.feed(&LIVE_STATUS[..15]);
        stream.feed(&LIVE_STATUS);
        expect_status(stream.next_message());
        assert!(stream.next_message().is_none());
    }

    #[test]
    fn test_corrupt_frame_then_complete_frame() {
        let mut stream = StreamParser::new();
        let mut corrupted = LIVE_STATUS;
        corrupted[12] = 0x3C; // flip a body byte; CRC no longer matches

        stream.feed(&corrupted);
        stream.feed(&LIVE_STATUS);

        match stream.next_message() {
            Some(Err(ParseError::CrcError(_))) => {}
            other => panic!(
                "expected CRC error for corrupted frame, got {:?}",
                other.map(|r| r.is_ok())
            ),
        }
        // The good frame after it still parses
        expect_status(stream.next_message());
    }

    #[test]
    fn test_oversized_garbage_is_bounded() {
        let mut stream = StreamParser::new();
        // A SOR followed by endless non-EOR data: buffer must not grow forever
        stream.feed(&[SOR]);
        let junk = vec![0x00u8; MAX_BUFFER + 1];
        stream.feed(&junk);
        assert!(stream.next_message().is_none());
        assert!(stream.buffer.is_empty(), "oversized buffer should be dropped");

        // Parser recovers afterwards
        stream.feed(&LIVE_STATUS);
        expect_status(stream.next_message());
    }
}
