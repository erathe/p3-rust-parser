//! Integration tests validating message generation against live decoder captures.
//!
//! These tests ensure that our message builder produces byte-perfect output
//! matching real MyLaps ProChip decoder P3 protocol messages captured during
//! live BMX racing on 2025-10-30.
//!
//! Each test:
//! 1. Builds a message with exact field values from the capture
//! 2. Compares generated output byte-for-byte with the original capture
//! 3. Validates the CRC independently
//!
//! Test coverage includes:
//! - STATUS messages (clean and noisy conditions)
//! - PASSING rider messages (strong, medium, weak signals)
//! - PASSING gate messages (two different beacon IDs)

use p3_test_server::generator::builder::{
    build_gate_passing, build_rider_passing, build_status, format_timestamp,
};
use p3_test_server::generator::tlv;
use p3_protocol::validate_crc;
use std::fs;

/// Test STATUS message with clean signal conditions.
///
/// Fixture: captured_message_001.bin
/// - Noise: 53 (lowest captured)
/// - Temperature: 1.6°C (16 tenths)
/// - GPS Status: 1 (locked)
/// - Satellites: 0
/// - Decoder ID: 0x000C00D0 (D0000C00)
#[test]
fn test_generate_status_clean_signal() {
    // Build message with exact field values from capture
    let generated = build_status(
        53,         // noise
        16,         // temperature (1.6°C)
        1,          // gps_status (locked)
        0,          // satinuse
        0x000C00D0, // extended
    );

    // Load expected fixture
    let expected = fs::read("../tests/fixtures/live_capture/captured_message_001.bin")
        .expect("Failed to read fixture");

    // Validate CRC independently
    assert!(
        validate_crc(&generated).expect("CRC validation failed"),
        "Generated message has invalid CRC"
    );

    // Byte-perfect comparison
    assert_eq!(
        generated, expected,
        "Generated STATUS message does not match live capture\n\
         Generated: {:02X?}\n\
         Expected:  {:02X?}",
        generated, expected
    );
}

/// Test STATUS message with noisy signal conditions.
///
/// Fixture: captured_message_041.bin
/// - Noise: 62 (highest captured)
/// - Temperature: 1.5°C (15 tenths)
/// - GPS Status: 1 (locked)
/// - Satellites: 0
/// - Decoder ID: 0x000C00D0 (D0000C00)
#[test]
fn test_generate_status_noisy_signal() {
    // Build message with exact field values from capture
    let generated = build_status(
        62,         // noise
        15,         // temperature (1.5°C)
        1,          // gps_status (locked)
        0,          // satinuse
        0x000C00D0, // extended
    );

    // Load expected fixture
    let expected = fs::read("../tests/fixtures/live_capture/captured_message_041.bin")
        .expect("Failed to read fixture");

    // Validate CRC independently
    assert!(
        validate_crc(&generated).expect("CRC validation failed"),
        "Generated message has invalid CRC"
    );

    // Byte-perfect comparison
    assert_eq!(
        generated, expected,
        "Generated STATUS message does not match live capture\n\
         Generated: {:02X?}\n\
         Expected:  {:02X?}",
        generated, expected
    );
}

/// Test PASSING rider message with excellent signal quality.
///
/// Fixture: captured_message_073.bin
/// - Passing Number: 8,841
/// - Transponder: 102758186 (0x061FF72A)
/// - String: "FL-94890"
/// - Strength: 127
/// - Hits: 33 (highest captured)
/// - RTC Time: 0x00064265EC300635
/// - Decoder ID: 0x000C00D0 (D0000C00)
#[test]
fn test_generate_rider_high_hits() {
    let string = b"FL-94890";

    // Build message with exact field values from capture
    let generated = build_rider_passing(
        8841,               // passing_number
        102758186,          // transponder (0x061FF72A)
        string,             // string identifier
        0x00064265EC300635, // rtc_time
        127,                // strength
        33,                 // hits
    ).expect("Failed to build message");

    // Load expected fixture
    let expected = fs::read("../tests/fixtures/live_capture/captured_message_073.bin")
        .expect("Failed to read fixture");

    // Validate CRC independently
    assert!(
        validate_crc(&generated).expect("CRC validation failed"),
        "Generated message has invalid CRC"
    );

    // Byte-perfect comparison
    assert_eq!(
        generated, expected,
        "Generated PASSING message does not match live capture\n\
         Generated: {:02X?}\n\
         Expected:  {:02X?}",
        generated, expected
    );
}

/// Test PASSING rider message with peak signal strength.
///
/// Fixture: captured_message_017.bin
/// - Passing Number: 8,857
/// - Transponder: 102758186 (0x061FF72A)
/// - String: "FL-94890"
/// - Strength: 133 (highest captured)
/// - Hits: 29
/// - RTC Time: 0x000642660 8CA0185
/// - Decoder ID: 0x000C00D0 (D0000C00)
#[test]
fn test_generate_rider_peak_strength() {
    let string = b"FL-94890";

    // Build message with exact field values from capture
    let generated = build_rider_passing(
        8857,               // passing_number
        102758186,          // transponder (0x061FF72A)
        string,             // string identifier
        0x0006426608CA0185, // rtc_time
        133,                // strength
        29,                 // hits
    ).expect("Failed to build message");

    // Load expected fixture
    let expected = fs::read("../tests/fixtures/live_capture/captured_message_017.bin")
        .expect("Failed to read fixture");

    // Validate CRC independently
    assert!(
        validate_crc(&generated).expect("CRC validation failed"),
        "Generated message has invalid CRC"
    );

    // Byte-perfect comparison
    assert_eq!(
        generated, expected,
        "Generated PASSING message does not match live capture\n\
         Generated: {:02X?}\n\
         Expected:  {:02X?}",
        generated, expected
    );
}

/// Test PASSING rider message with weak signal.
///
/// Fixture: captured_message_025.bin
/// - Passing Number: 8,861
/// - Transponder: 102758186 (0x061FF72A)
/// - String: "FL-94890"
/// - Strength: 76 (weak)
/// - Hits: 2 (minimal detection)
/// - RTC Time: 0x000642660AF69629
/// - Decoder ID: 0x000C00D0 (D0000C00)
#[test]
fn test_generate_rider_weak_signal() {
    let string = b"FL-94890";

    // Build message with exact field values from capture
    let generated = build_rider_passing(
        8861,               // passing_number
        102758186,          // transponder (0x061FF72A)
        string,             // string identifier
        0x000642660AF69629, // rtc_time
        76,                 // strength
        2,                  // hits
    ).expect("Failed to build message");

    // Load expected fixture
    let expected = fs::read("../tests/fixtures/live_capture/captured_message_025.bin")
        .expect("Failed to read fixture");

    // Validate CRC independently
    assert!(
        validate_crc(&generated).expect("CRC validation failed"),
        "Generated message has invalid CRC"
    );

    // Byte-perfect comparison
    assert_eq!(
        generated, expected,
        "Generated PASSING message does not match live capture\n\
         Generated: {:02X?}\n\
         Expected:  {:02X?}",
        generated, expected
    );
}

/// Test PASSING gate message with primary beacon ID.
///
/// Fixture: captured_message_008.bin
/// - Passing Number: 8,855
/// - Transponder: 9,992 (reserved gate beacon)
/// - No STRING field (distinguishes gates from riders)
/// - Strength: 0
/// - Hits: 0
/// - RTC Time: 0x0006426606711F54
/// - Decoder ID: 0x000C00D0 (D0000C00)
#[test]
fn test_generate_gate_primary() {
    // Build message with exact field values from capture
    let generated = build_gate_passing(
        8855,               // passing_number
        9992,               // transponder (gate beacon)
        0x0006426606711F54, // rtc_time
    );

    // Load expected fixture
    let expected = fs::read("../tests/fixtures/live_capture/captured_message_008.bin")
        .expect("Failed to read fixture");

    // Validate CRC independently
    assert!(
        validate_crc(&generated).expect("CRC validation failed"),
        "Generated message has invalid CRC"
    );

    // Byte-perfect comparison
    assert_eq!(
        generated, expected,
        "Generated PASSING gate message does not match live capture\n\
         Generated: {:02X?}\n\
         Expected:  {:02X?}",
        generated, expected
    );
}

/// Test PASSING gate message with alternative beacon ID.
///
/// Fixture: captured_message_024.bin
/// - Passing Number: 8,859
/// - Transponder: 9,995 (reserved gate beacon)
/// - No STRING field
/// - Strength: 0
/// - Hits: 0
/// - RTC Time: 0x000642660ACF34E8
/// - Decoder ID: 0x000C00D0 (D0000C00)
#[test]
fn test_generate_gate_alternative() {
    // Build message with exact field values from capture
    let generated = build_gate_passing(
        8859,               // passing_number
        9995,               // transponder (gate beacon)
        0x000642660ACF34E8, // rtc_time
    );

    // Load expected fixture
    let expected = fs::read("../tests/fixtures/live_capture/captured_message_024.bin")
        .expect("Failed to read fixture");

    // Validate CRC independently
    assert!(
        validate_crc(&generated).expect("CRC validation failed"),
        "Generated message has invalid CRC"
    );

    // Byte-perfect comparison
    assert_eq!(
        generated, expected,
        "Generated PASSING gate message does not match live capture\n\
         Generated: {:02X?}\n\
         Expected:  {:02X?}",
        generated, expected
    );
}

/// Test timestamp extraction and decoding from live capture files.
///
/// This test demonstrates that our timestamp encoding/decoding is correct by:
/// 1. Reading real capture files
/// 2. Extracting the RTC_TIME field bytes
/// 3. Decoding to microseconds
/// 4. Converting to human-readable format
/// 5. Re-encoding and verifying byte-perfect match
#[test]
fn test_timestamp_decode_and_display() {
    println!("\n========================================");
    println!("RTC TIMESTAMP ANALYSIS FROM LIVE CAPTURES");
    println!("Captured on 2025-10-30 during live BMX racing");
    println!("Note: These are RTC (decoder internal clock) timestamps, not GPS-synchronized UTC");
    println!("========================================\n");

    // Test data: (filename, rtc_time value from test, description)
    let test_cases = [
        (
            "captured_message_073.bin",
            0x00064265EC300635,
            "Passing 8,841 - High hits (33), Strength 127",
        ),
        (
            "captured_message_017.bin",
            0x0006426608CA0185,
            "Passing 8,857 - Peak strength (133), Hits 29",
        ),
        (
            "captured_message_025.bin",
            0x000642660AF69629,
            "Passing 8,861 - Weak signal (76), Hits 2",
        ),
        (
            "captured_message_008.bin",
            0x0006426606711F54, // 1761855822438228 microseconds
            "Passing 8,855 - Gate beacon 9992",
        ),
        (
            "captured_message_024.bin",
            0x000642660ACF34E8, // 1761855895713000 microseconds
            "Passing 8,859 - Gate beacon 9995",
        ),
    ];

    let mut timestamps = Vec::new();

    for (filename, expected_timestamp, description) in &test_cases {
        println!("{}", description);
        println!("  File: {}", filename);

        // Read the capture file
        let data = fs::read(format!("../tests/fixtures/live_capture/{}", filename))
            .expect("Failed to read capture file");

        // Find the RTC_TIME field (Tag 0x04) in the message
        // Properly parse TLV fields after the header
        let mut found_timestamp = None;
        let mut offset = 10; // Skip header: SOR(1) + VERSION(1) + LENGTH(2) + CRC(2) + RESERVED(2) + TYPE(2)

        // Parse TLV fields until we find RTC_TIME or hit EOR
        while offset < data.len() - 1 {
            let tag = data[offset];

            // Check for EOR marker
            if tag == 0x8F {
                break;
            }

            // Read length
            if offset + 1 >= data.len() {
                break;
            }
            let length = data[offset + 1] as usize;

            // Check if this is RTC_TIME tag
            if tag == 0x04 && length == 8 && offset + 2 + length <= data.len() {
                let timestamp_bytes = &data[offset + 2..offset + 2 + length];
                found_timestamp = Some(u64::from_le_bytes(
                    timestamp_bytes.try_into().expect("Invalid timestamp bytes"),
                ));
                break;
            }

            // Move to next TLV field
            offset += 2 + length;
        }

        let timestamp = found_timestamp.expect("RTC_TIME field not found in capture");

        // Verify it matches our expected value
        assert_eq!(
            timestamp, *expected_timestamp,
            "Extracted timestamp doesn't match expected value"
        );

        // Format and display
        let formatted = format_timestamp(timestamp);
        println!("  RTC Time: {} (0x{:016X})", formatted, timestamp);
        println!("  Microseconds: {}", timestamp);

        // Verify re-encoding produces same bytes
        let re_encoded = tlv::encode_u64(0x04, timestamp);
        let original_tlv = &data[offset..offset + 10]; // Tag + Length + 8 bytes
        assert_eq!(
            &re_encoded[..],
            original_tlv,
            "Re-encoded timestamp doesn't match original"
        );
        println!("  ✓ Re-encoding verified byte-perfect");
        println!();

        timestamps.push((description, timestamp));
    }

    // Calculate and display time deltas
    println!("========================================");
    println!("TIME DELTAS BETWEEN PASSINGS");
    println!("========================================\n");

    let mut sorted_timestamps = timestamps.clone();
    sorted_timestamps.sort_by_key(|&(_, ts)| ts);

    for i in 1..sorted_timestamps.len() {
        let (prev_desc, prev_ts) = sorted_timestamps[i - 1];
        let (curr_desc, curr_ts) = sorted_timestamps[i];
        let delta_micros = curr_ts - prev_ts;
        let delta_secs = delta_micros as f64 / 1_000_000.0;

        println!("{} ->", prev_desc);
        println!("{}", curr_desc);
        println!(
            "  Delta: {:.6} seconds ({} microseconds)\n",
            delta_secs, delta_micros
        );
    }
}
