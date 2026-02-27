use crate::{Message, ParseError, Parser};
use p3_protocol::{ESCAPE, SOR};

/// Parsed-message output from [`MessageFramer::feed`].
pub type FrameResult = Result<Message, ParseError>;

/// Accumulates bytes from a TCP stream and yields complete parsed P3 messages.
///
/// Handles escape-sequence-aware framing: the LENGTH field in the P3 header
/// uses unescaped byte count while wire bytes may include escape sequences.
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

    /// Feed raw bytes and parse any complete frames now available.
    pub fn feed(&mut self, data: &[u8]) -> Vec<FrameResult> {
        self.buffer.extend_from_slice(data);

        let mut results = Vec::new();
        while let Some(message_end) = find_complete_message(&self.buffer) {
            let message_data = &self.buffer[..message_end];
            results.push(self.parser.parse(message_data));
            self.buffer.drain(..message_end);
        }
        results
    }
}

impl Default for MessageFramer {
    fn default() -> Self {
        Self::new()
    }
}

/// Calculate the escaped-buffer end position for a frame of `unescaped_length`.
pub fn calculate_escaped_message_end(
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

/// Find the end byte position of the next complete message in `buffer`.
pub fn find_complete_message(buffer: &[u8]) -> Option<usize> {
    let sor_pos = buffer.iter().position(|&b| b == SOR)?;

    if buffer.len() < sor_pos + 4 {
        return None;
    }

    let len_bytes = [buffer[sor_pos + 2], buffer[sor_pos + 3]];
    let unescaped_length = u16::from_le_bytes(len_bytes) as usize;

    calculate_escaped_message_end(buffer, sor_pos, unescaped_length)
}
