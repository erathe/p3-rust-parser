# BMX Race Timing — Development Commands

# Default: show available recipes
default:
    @just --list

# Run all tests across the workspace
test:
    cargo test

# Build the entire workspace
build:
    cargo build

# Start the P3 test server (decoder simulator) with full-race scenario
test-server riders="6":
    cargo run -p p3-test-server -- --scenario full-race --riders {{riders}}

# Start the P3 test server in idle mode (STATUS messages only)
test-server-idle:
    cargo run -p p3-test-server -- --scenario idle

# Start the p3-server (Axum backend on :3001)
server:
    cargo run -p p3-server

# Start the p3-server with no decoder connection (API/WebSocket only)
server-no-decoder:
    cargo run -p p3-server -- --no-decoder

# Start a track-side client that decodes local P3 and forwards JSON to central server
track-client client_id track_id session_id="dev-default" decoder_host="localhost" decoder_port="5403" central_url="http://localhost:3001":
    cargo run -p p3-track-client -- \
      --client-id {{client_id}} \
      --track-id {{track_id}} \
      --session-id {{session_id}} \
      --decoder-host {{decoder_host}} \
      --decoder-port {{decoder_port}} \
      --central-base-url {{central_url}}

# Start the SvelteKit frontend dev server
frontend:
    cd frontend && npm run dev

# Install frontend dependencies
frontend-install:
    cd frontend && npm install

# Run everything for development (3 terminals needed)
# Terminal 1: just test-server
# Terminal 2: just server
# Terminal 3: just frontend

# Start test-server + p3-server together (frontend needs a separate terminal)
dev:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Starting test-server (full-race, 6 riders) on :5403..."
    cargo run -p p3-test-server -- --scenario full-race --riders 6 &
    TEST_PID=$!
    sleep 2
    echo "Starting p3-server on :3001..."
    cargo run -p p3-server &
    SERVER_PID=$!
    echo ""
    echo "Backend running. Start frontend in another terminal:"
    echo "  just frontend"
    echo ""
    echo "Press Ctrl+C to stop all..."
    trap "kill $TEST_PID $SERVER_PID 2>/dev/null" EXIT INT TERM
    wait
