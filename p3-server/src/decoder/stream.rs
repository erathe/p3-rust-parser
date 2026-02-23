use p3_parser::{Message, Parser};
use p3_protocol::{ESCAPE, SOR};

/// Accumulates bytes from a TCP stream and yields complete parsed P3 messages.
///
/// Handles the P3 protocol's escape-sequence-aware framing: the LENGTH field
/// in the header represents the *unescaped* length, but the wire format contains
/// escape sequences (0x8D prefix) that add extra bytes.
pub struct MessageFramer {
    buffer: Vec<u8>,
    parser: Parser,
}

impl MessageFramer {
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(4096),
            parser: Parser::new(),
        }
    }

    /// Feed raw bytes from the TCP stream. Returns all complete messages
    /// that can be parsed from the accumulated buffer.
    pub fn feed(&mut self, data: &[u8]) -> Vec<FrameResult> {
        self.buffer.extend_from_slice(data);

        let mut results = Vec::new();

        while let Some(message_end) = find_complete_message(&self.buffer) {
            let message_data = &self.buffer[..message_end];

            match self.parser.parse(message_data) {
                Ok(message) => results.push(Ok(message)),
                Err(e) => results.push(Err(e)),
            }

            self.buffer.drain(..message_end);
        }

        results
    }
}

pub type FrameResult = Result<Message, p3_parser::ParseError>;

/// Calculate the actual buffer position where a message ends.
///
/// The LENGTH field in the P3 protocol represents the UNESCAPED length.
/// Each escape sequence (0x8D 0xXX) counts as 1 unescaped byte but takes 2 buffer bytes.
fn calculate_escaped_message_end(
    buffer: &[u8],
    start_pos: usize,
    unescaped_length: usize,
) -> Option<usize> {
    let mut buffer_pos = start_pos;
    let mut unescaped_count = 0;

    while unescaped_count < unescaped_length {
        if buffer_pos >= buffer.len() {
            return None;
        }

        if buffer[buffer_pos] == ESCAPE {
            if buffer_pos + 1 >= buffer.len() {
                return None;
            }
            buffer_pos += 2;
            unescaped_count += 1;
        } else {
            buffer_pos += 1;
            unescaped_count += 1;
        }
    }

    Some(buffer_pos)
}

/// Find the end position of a complete message in the buffer.
///
/// Returns the byte position after the complete message if one is available.
fn find_complete_message(buffer: &[u8]) -> Option<usize> {
    let sor_pos = buffer.iter().position(|&b| b == SOR)?;

    // Need at least SOR + VERSION + LENGTH (4 bytes) to determine size
    if buffer.len() < sor_pos + 4 {
        return None;
    }

    let len_bytes = [buffer[sor_pos + 2], buffer[sor_pos + 3]];
    let unescaped_length = u16::from_le_bytes(len_bytes) as usize;

    calculate_escaped_message_end(buffer, sor_pos, unescaped_length)
}
