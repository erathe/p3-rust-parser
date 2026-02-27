# P3 BMX Parser - P3 Protocol

A Rust workspace for working with the P3 binary protocol used in BMX racing timing systems.

## Project Structure

This is a Cargo workspace containing multiple crates:

### p3-protocol (Shared Library)

Core protocol definitions shared by both the test server and parser.

**Provides:**
- Frame constants (SOR, EOR, ESCAPE)
- Message types (PASSING, STATUS, VERSION)
- TLV field definitions
- Escape/unescape functions
- CRC calculation and validation

### 🧪 p3-test-server (Decoder Simulator)

Simulates a MyLaps ProChip Smart Decoder for testing parsers without physical hardware.

**Features:**
- Byte-perfect message generation (validated against live captures)
- Multiple race scenarios (BMX, idle, GPS loss, etc.)

### 🔍 p3-parser (Message Parser)

Parses binary P3 messages to structured data and JSON.

### 🌐 p3-server (Central Server)

Axum-based central timing server that receives decoder data and serves realtime APIs/WebSocket feeds.

### 📡 p3-track-client (Track-side Forwarder)

Track-local service that connects to physical/local decoder TCP, decodes P3 messages, and forwards normalized JSON to the central server ingest API.

**Will provide:**
- Frame parsing with CRC validation
- TLV decoding
- JSON serialization
- TCP/serial client support

## Quick Start

### Build the workspace

```bash
cargo build --workspace
```

### Run tests

```bash
cargo test --workspace
```

### Run test server (when implemented)

```bash
cargo run --bin test-server -- --scenario bmx-race --port 5403
```

## Documentation

- `docs/` - Comprehensive protocol documentation and analysis
- `docs/oracle-repository-structure-analysis.md` - Repository architecture analysis
- `tests/fixtures/` - Binary test messages (synthetic + live captures)
- `tests/expected/` - Expected parser outputs

## Protocol Overview

The P3 protocol is a binary protocol with:
- Little-endian byte order
- Escape sequences for control bytes (0x8A-0x8F)
- CRC-16-CCITT validation
- TLV (Tag-Length-Value) message bodies

**Key message types:**
- **PASSING** - Transponder detection (rider or gate beacon)
- **STATUS** - Decoder operational status
- **VERSION** - Hardware/firmware identification

## Development

### Adding a new crate

1. Create the crate directory
2. Add to `workspace.members` in root `Cargo.toml`
3. Use `workspace = true` for shared dependencies

### Running specific crate tests

```bash
cargo test -p p3-protocol
cargo test -p p3-test-server
cargo test -p p3-parser
```

## License

CC-BY-NC-4.0 (Creative Commons Attribution-NonCommercial 4.0 International)

This project is licensed for non-commercial use only. You are free to use, modify, and distribute this software for personal, educational, or research purposes, with attribution to the author. Commercial use is prohibited without explicit permission.
