use clap::Parser as ClapParser;
use p3_parser::{Message, Parser};
use p3_protocol::{ESCAPE, SOR};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

#[derive(ClapParser)]
#[command(name = "p3-parser")]
#[command(about = "Parse MyLaps ProChip P3 binary protocol messages to JSON")]
struct Args {
    #[arg(short = 'H', long, default_value = "localhost")]
    host: String,

    #[arg(short, long, default_value = "5403")]
    port: u16,

    #[arg(long)]
    pretty: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    eprintln!("Connecting to {}:{}...", args.host, args.port);

    let mut stream = TcpStream::connect((args.host.as_str(), args.port)).await?;
    eprintln!("Connected!");

    let parser = Parser::new();
    let mut buffer = Vec::new();

    loop {
        // Read data from stream
        let mut chunk = [0u8; 4096];
        let n = stream.read(&mut chunk).await?;

        if n == 0 {
            eprintln!("Connection closed");
            break;
        }

        buffer.extend_from_slice(&chunk[..n]);

        // Try to parse messages from buffer
        while let Some(message_end) = find_complete_message(&buffer) {
            let message_data = &buffer[..message_end];

            match parser.parse(message_data) {
                Ok(message) => match message {
                    Message::Passing(passing_message) => {
                        let json = if args.pretty {
                            serde_json::to_string_pretty(&passing_message)?
                        } else {
                            serde_json::to_string(&passing_message)?
                        };
                        println!("{}", json);
                    }
                    Message::Status(_status_message) => {}
                    Message::Version(_version_message) => {}
                },
                Err(e) => {
                    eprintln!("Parse error: {}", e);
                }
            }

            buffer.drain(..message_end);
        }
    }

    Ok(())
}

/// Calculate the actual buffer position where a message ends
///
/// The LENGTH field in the P3 protocol represents the UNESCAPED length.
/// This function scans through the escaped buffer to find where the message
/// actually ends, accounting for escape sequences.
///
/// Each escape sequence (0x8D 0xXX) counts as 1 unescaped byte but takes 2 buffer bytes.
///
/// Returns Some(buffer_pos) if the complete message is available, or None if incomplete.
fn calculate_escaped_message_end(
    buffer: &[u8],
    start_pos: usize,
    unescaped_length: usize,
) -> Option<usize> {
    let mut buffer_pos = start_pos;
    let mut unescaped_count = 0;

    while unescaped_count < unescaped_length {
        if buffer_pos >= buffer.len() {
            // Ran out of buffer before reaching the expected length
            return None;
        }

        if buffer[buffer_pos] == ESCAPE {
            // Need to check if there's a second byte for the escape sequence
            if buffer_pos + 1 >= buffer.len() {
                // Incomplete escape sequence
                return None;
            }
            // This is an escape sequence: 2 buffer bytes = 1 unescaped byte
            buffer_pos += 2;
            unescaped_count += 1;
        } else {
            // Regular byte: 1 buffer byte = 1 unescaped byte
            buffer_pos += 1;
            unescaped_count += 1;
        }
    }

    Some(buffer_pos)
}

/// Find the end position of a complete message in the buffer
///
/// Returns the position after EOR marker if found
fn find_complete_message(buffer: &[u8]) -> Option<usize> {
    // Find SOR marker
    let sor_pos = buffer.iter().position(|&b| b == SOR)?;

    // Need at least SOR + VERSION + LENGTH to determine message size
    if buffer.len() < sor_pos + 4 {
        return None;
    }

    // Read length field (bytes 2-3 after SOR, little-endian)
    // IMPORTANT: This is the UNESCAPED length, not the escaped length
    let len_bytes = [buffer[sor_pos + 2], buffer[sor_pos + 3]];
    let unescaped_length = u16::from_le_bytes(len_bytes) as usize;

    // Calculate where the message ends in the escaped buffer
    // We need to scan forward accounting for escape sequences
    // Returns None if the buffer doesn't contain the complete message
    calculate_escaped_message_end(buffer, sor_pos, unescaped_length)
}

#[cfg(test)]
mod tests {
    use super::*;
    use p3_protocol::{EOR, VERSION};

    #[test]
    fn test_calculate_escaped_message_end_no_escapes() {
        // Message with no escape sequences
        // [SOR, VERSION, LEN_LO, LEN_HI, ...data..., EOR]
        // Unescaped length = 10 bytes (including all header bytes and EOR)
        let buffer = vec![SOR, VERSION, 0x0A, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, EOR];

        let end_pos = calculate_escaped_message_end(&buffer, 0, 10);
        assert_eq!(end_pos, Some(10), "Should end at position 10 (no escapes)");
    }

    #[test]
    fn test_calculate_escaped_message_end_with_one_escape() {
        // Message with 1 escape sequence
        // Unescaped: [SOR, VERSION, LEN, LEN, DATA, 0x8F, EOR] = 7 bytes
        // Escaped:   [SOR, VERSION, LEN, LEN, DATA, 0x8D, 0xAF, EOR] = 8 bytes
        let buffer = vec![SOR, VERSION, 0x07, 0x00, 0x01, 0x8D, 0xAF, EOR];

        let end_pos = calculate_escaped_message_end(&buffer, 0, 7);
        assert_eq!(
            end_pos,
            Some(8),
            "Should account for escape sequence (7 unescaped = 8 buffer bytes)"
        );
    }

    #[test]
    fn test_calculate_escaped_message_end_with_multiple_escapes() {
        // Message with 3 escape sequences
        // Unescaped length = 8, but takes 11 buffer bytes due to 3 escapes
        let buffer = vec![
            SOR, VERSION, 0x08, 0x00, 0x8D, 0xAA, // Escape for 0x8A
            0x8D, 0xAE, // Escape for 0x8E
            0x8D, 0xAF, // Escape for 0x8F
            EOR,
        ];

        let end_pos = calculate_escaped_message_end(&buffer, 0, 8);
        assert_eq!(
            end_pos,
            Some(11),
            "Should account for 3 escape sequences (8 unescaped = 11 buffer bytes)"
        );
    }

    #[test]
    fn test_find_complete_message_no_escapes() {
        // Complete message with no escapes
        // LENGTH = 6 means total of 6 bytes in unescaped message
        let buffer = vec![SOR, VERSION, 0x06, 0x00, 0x01, EOR];

        let end = find_complete_message(&buffer);
        assert_eq!(end, Some(6), "Should find complete message");
    }

    #[test]
    fn test_find_complete_message_with_escape() {
        // Complete message with 1 escape (LENGTH=6, but 7 buffer bytes due to escape)
        // Unescaped: [SOR, VERSION, LEN_LO, LEN_HI, 0x8F, EOR] = 6 bytes
        // Escaped:   [SOR, VERSION, LEN_LO, LEN_HI, 0x8D, 0xAF, EOR] = 7 bytes
        let buffer = vec![SOR, VERSION, 0x06, 0x00, 0x8D, 0xAF, EOR];

        let end = find_complete_message(&buffer);
        assert_eq!(
            end,
            Some(7),
            "Should find complete message accounting for escape"
        );
    }

    #[test]
    fn test_find_complete_message_incomplete() {
        // Incomplete message (says length 10 but only has 5 bytes)
        let buffer = vec![SOR, VERSION, 0x0A, 0x00, 0x01];

        let end = find_complete_message(&buffer);
        assert_eq!(end, None, "Should return None for incomplete message");
    }

    #[test]
    fn test_find_complete_message_incomplete_with_escape() {
        // Message with LENGTH=7 but incomplete escape sequence at end
        // Needs 7 unescaped bytes, has escape at position 4 but missing second byte
        let buffer = vec![SOR, VERSION, 0x07, 0x00, 0x8D];

        let end = find_complete_message(&buffer);
        assert_eq!(
            end, None,
            "Should return None when escape sequence incomplete"
        );
    }

    #[test]
    fn test_find_complete_message_no_sor() {
        // Buffer without SOR marker
        let buffer = vec![0x01, 0x02, 0x03];

        let end = find_complete_message(&buffer);
        assert_eq!(end, None, "Should return None when no SOR found");
    }

    #[test]
    fn test_find_complete_message_too_short() {
        // Buffer with SOR but too short to read length field
        let buffer = vec![SOR, VERSION];

        let end = find_complete_message(&buffer);
        assert_eq!(end, None, "Should return None when buffer too short");
    }

    #[test]
    fn test_multiple_consecutive_messages() {
        // Two complete messages back-to-back, second one has escape
        // Message 1: [SOR, VERSION, 6, 0, DATA, EOR] = 6 bytes, no escapes
        // Message 2: [SOR, VERSION, 6, 0, 0x8D, 0xAF, EOR] = 7 bytes (escaped), 6 bytes (unescaped)
        let buffer = vec![
            SOR, VERSION, 0x06, 0x00, 0x01, EOR, SOR, VERSION, 0x06, 0x00, 0x8D, 0xAF, EOR,
        ];

        // Find first message
        let end1 = find_complete_message(&buffer);
        assert_eq!(end1, Some(6), "Should find first message");

        // Simulate draining first message
        let remaining = &buffer[6..];
        let end2 = find_complete_message(remaining);
        assert_eq!(end2, Some(7), "Should find second message with escape");
    }

    #[test]
    fn test_gate_drop_scenario_with_escape() {
        // Simulate real gate drop scenario that caused the bug
        // LENGTH=43 (0x2B) with 1 escape sequence somewhere in the data
        // This means 43 bytes unescaped, 44 bytes escaped
        let mut buffer = vec![SOR, VERSION, 0x2B, 0x00];

        // Add 38 bytes of normal data (43 total - 4 header - 1 that will be escaped)
        for i in 0..38 {
            buffer.push(i as u8);
        }

        // Add 1 byte that needs escaping (0x8F) - this becomes 2 bytes in escaped form
        buffer.push(0x8D);
        buffer.push(0xAF);

        // Total buffer: 4 (header) + 38 (data) + 2 (escaped byte) = 44 bytes
        // Unescaped length in header = 43

        let end = find_complete_message(&buffer);
        assert_eq!(
            end,
            Some(44),
            "Should correctly handle gate drop with escape sequence"
        );

        let sor_pos = 0;
        let buggy_end = sor_pos + 43;
        assert_eq!(
            buggy_end, 43,
            "Old buggy code would have stopped at position 43"
        );
        assert_ne!(buggy_end, 44, "Old code would miss the last byte");
    }
}
