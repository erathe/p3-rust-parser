//! StreamParser integration tests.
//!
//! Two sources of truth:
//! 1. Raw live capture streams from decoder D0000C00 (2025-10-30) — the
//!    parser must extract every message regardless of how the bytes are
//!    chunked.
//! 2. Round-trips against the p3-test-server generator, whose output is
//!    byte-perfect-validated against the same live captures.

use p3_parser::{Message, StreamParser};
use p3_test_server::generator::builder::{
    build_gate_passing_with_escape, build_rider_passing, build_status, build_version,
};
use std::fs;

/// Feed `data` to a fresh StreamParser in chunks of `chunk_size` and collect
/// successfully parsed messages, panicking on any parse error.
fn parse_stream(data: &[u8], chunk_size: usize) -> Vec<Message> {
    let mut stream = StreamParser::new();
    let mut messages = Vec::new();

    for chunk in data.chunks(chunk_size) {
        stream.feed(chunk);
        while let Some(result) = stream.next_message() {
            messages.push(result.expect("live capture frame should parse"));
        }
    }

    messages
}

#[test]
fn raw_live_captures_parse_identically_regardless_of_chunking() {
    for filename in [
        "../captures/mylaps_raw_data.bin",
        "../captures/mylaps_raw_data2.bin",
        "../captures/mylaps_raw_data3.bin",
    ] {
        let data = fs::read(filename).expect("failed to read raw capture");

        let whole = parse_stream(&data, data.len());
        assert!(
            !whole.is_empty(),
            "{}: expected at least one message",
            filename
        );

        // Byte-by-byte delivery must yield the exact same messages
        let byte_by_byte = parse_stream(&data, 1);
        assert_eq!(
            whole, byte_by_byte,
            "{}: chunking changed parse results",
            filename
        );

        // And an awkward chunk size that splits frames mid-header
        let chunked = parse_stream(&data, 7);
        assert_eq!(whole, chunked, "{}: chunking changed parse results", filename);
    }
}

#[test]
fn generated_race_traffic_round_trips() {
    // A realistic burst: status, gate drop (with escape sequence), two riders
    let mut data = Vec::new();
    data.extend(build_status(53, 16, 1, 0, 0x000C00D0));
    data.extend(build_gate_passing_with_escape(8855, 9992));
    data.extend(
        build_rider_passing(8856, 102758186, b"FL-94890", 1762286699916839, 127, 33).unwrap(),
    );
    data.extend(
        build_rider_passing(8857, 123456789, b"FL-12345", 1762286700916839, 120, 45).unwrap(),
    );

    let messages = parse_stream(&data, 5);
    assert_eq!(messages.len(), 4);

    match &messages[1] {
        Message::Passing(gate) => {
            assert_eq!(gate.transponder_id, 9992);
            assert_eq!(gate.rtc_time_us, 1762286699916839);
            assert!(gate.transponder_string.is_none());
        }
        other => panic!("expected gate PASSING, got {:?}", other),
    }
    match &messages[2] {
        Message::Passing(rider) => {
            assert_eq!(rider.transponder_id, 102758186);
            assert_eq!(rider.transponder_string.as_deref(), Some("FL-94890"));
        }
        other => panic!("expected rider PASSING, got {:?}", other),
    }
}

/// Messages whose unescaped LENGTH falls in 0x8A-0x8F (138-143 bytes) carry
/// an escaped LENGTH byte on the wire. Framing that reads LENGTH from the raw
/// buffer breaks on these; SOR/EOR-scan framing must not.
#[test]
fn frames_with_escaped_length_field_parse() {
    for total_len in 138usize..=143 {
        // VERSION message size = 29 + description + version_string bytes
        let desc_len = total_len - 29 - 5;
        let description = "x".repeat(desc_len);
        let message = build_version(0x000C00D0, &description, "1.0.0", 100).unwrap();

        let messages = parse_stream(&message, 3);
        assert_eq!(
            messages.len(),
            1,
            "message with unescaped length {} (0x{:02X}) failed to frame",
            total_len,
            total_len
        );
        match &messages[0] {
            Message::Version(version) => {
                assert_eq!(version.description, description);
            }
            other => panic!("expected VERSION, got {:?}", other),
        }
    }
}
