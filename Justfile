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
# Requires a NATS JetStream instance on nats://127.0.0.1:4222.
server:
    cargo run -p p3-server

# Start the p3-server with no decoder connection (API/WebSocket only)
# Requires a NATS JetStream instance on nats://127.0.0.1:4222.
server-no-decoder:
    cargo run -p p3-server -- --no-decoder

# Start local NATS with JetStream and monitoring
nats:
    docker run --rm -it -p 4222:4222 -p 8222:8222 nats:2.11-alpine -js -m 8222

# Start a track-side client that decodes local P3 and forwards JSON to central server
track-client client_id track_id decoder_host="localhost" decoder_port="5403" central_url="http://localhost:3001":
    cargo run -p p3-track-client -- \
      --client-id {{client_id}} \
      --track-id {{track_id}} \
      --decoder-host {{decoder_host}} \
      --decoder-port {{decoder_port}} \
      --central-base-url {{central_url}}

# Run live onboarding feed (test-server -> track-client -> central server)
# Use with `just server-no-decoder` in another terminal for track-scoped ingest-only testing.
onboarding-feed client_id track_id riders="6" central_url="http://localhost:3001":
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Starting p3-test-server (full-race, {{riders}} riders) on :5403..."
    cargo run -p p3-test-server -- --scenario full-race --riders {{riders}} &
    TEST_PID=$!
    sleep 2
    echo "Starting p3-track-client (track_id={{track_id}}, client_id={{client_id}})..."
    cargo run -p p3-track-client -- \
      --client-id {{client_id}} \
      --track-id {{track_id}} \
      --decoder-host localhost \
      --decoder-port 5403 \
      --central-base-url {{central_url}} &
    CLIENT_PID=$!
    echo ""
    echo "Live onboarding feed is running."
    echo "Open /admin/tracks/{{track_id}}/onboarding in the frontend."
    echo "Press Ctrl+C to stop both processes."
    trap "kill $TEST_PID $CLIENT_PID 2>/dev/null" EXIT INT TERM
    wait

# Start the SvelteKit frontend dev server
frontend:
    cd frontend && npm run dev

# Install frontend dependencies
frontend-install:
    cd frontend && npm install

# Build and start the local Docker stack in background
stack-up:
    #!/usr/bin/env bash
    set -euo pipefail
    if docker compose version >/dev/null 2>&1; then
      docker compose up --build -d
    else
      docker-compose up --build -d
    fi

# Stop and remove the local Docker stack
stack-down:
    #!/usr/bin/env bash
    set -euo pipefail
    if docker compose version >/dev/null 2>&1; then
      docker compose down
    else
      docker-compose down
    fi

# Tail logs from the local Docker stack
stack-logs:
    #!/usr/bin/env bash
    set -euo pipefail
    if docker compose version >/dev/null 2>&1; then
      docker compose logs -f
    else
      docker-compose logs -f
    fi

# Run a lightweight harness smoke check against the stack
stack-smoke:
    ./scripts/harness/smoke.sh

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
