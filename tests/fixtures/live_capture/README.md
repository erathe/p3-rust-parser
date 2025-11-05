# Live Capture Test Fixtures

These binary files contain real P3 protocol messages captured from a MyLaps ProChip decoder during live BMX racing on 2025-10-30 at 192.168.1.51.

## Selected Messages

### STATUS Messages (2)

**captured_message_001.bin**
- Noise: 53 (lowest noise level)
- Temperature: 1.6°C (warmer)
- CRC: Valid
- Purpose: Represents clean signal conditions

**captured_message_041.bin**
- Noise: 62 (highest noise level captured)
- Temperature: 1.5°C (cooler)
- CRC: Valid
- Purpose: Represents noisier signal conditions

### PASSING Rider Messages (3)

All messages from transponder ID 102758186 with string identifier "FL-94890"

**captured_message_073.bin**
- Passing Number: 8,841
- Signal Strength: 127
- Hits: 33
- CRC: Valid
- Purpose: Excellent signal quality, highest hit count

**captured_message_017.bin**
- Passing Number: 8,857
- Signal Strength: 133 (best signal)
- Hits: 29
- CRC: Valid
- Purpose: Best signal strength

**captured_message_025.bin**
- Passing Number: 8,861
- Signal Strength: 76 (weak)
- Hits: 2 (minimal)
- CRC: Valid
- Purpose: Weak signal conditions, edge case

### PASSING Gate Messages (2)

**captured_message_008.bin**
- Passing Number: 8,855
- Transponder ID: 9,992 (gate start beacon)
- CRC: Valid
- Purpose: Primary gate drop beacon

**captured_message_024.bin**
- Passing Number: 8,859
- Transponder ID: 9,995 (alternative gate beacon)
- CRC: Valid
- Purpose: Alternative gate beacon ID

## Archive

All other captured messages (91 total) have been moved to `captures/parsed/archive/`:
- 86 additional STATUS messages
- 5 additional PASSING gate messages (including duplicates)

## Analysis

These messages were analyzed using the Rust CLI tool at `test-server/src/bin/analyze_messages.rs`, which:
- Validates CRC-16-CCITT checksums (all 98 messages passed)
- Parses TLV-encoded fields
- Distinguishes between rider and gate transponders based on Tag 0x0A presence
- Extracts key metrics (signal strength, hits, noise, temperature)

Run the analyzer: `cargo run --bin analyze-messages <directory>`
