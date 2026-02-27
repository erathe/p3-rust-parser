# AGENTS.md

## Cursor Cloud specific instructions

### Project overview

P3 BMX Race Timing — a Rust + SvelteKit app for real-time BMX race timing using the MyLaps ProChip P3 binary protocol. Cargo workspace with 4 crates (`p3-protocol`, `p3-parser`, `p3-test-server`, `p3-server`) plus a SvelteKit frontend in `frontend/`.

### Prerequisites

- **Rust 1.85+** required (all crates use `edition = "2024"`). Run `rustup update stable` if needed.
- **Node.js 22+** and **npm** for the frontend.
- **just** task runner (`cargo install just`). Dev recipes are in `Justfile`.

### Key commands

See `Justfile` for all dev recipes. Summary:

| Task | Command |
|---|---|
| Build all Rust crates | `cargo build --workspace` |
| Run all Rust tests | `cargo test --workspace` |
| Frontend type-check | `cd frontend && npm run check` |
| Install frontend deps | `cd frontend && npm install` |

### Running the full stack (3 processes)

1. **Decoder simulator** (TCP on `:5403`): `just test-server`
2. **Axum backend** (HTTP/WS on `:3001`): `just server`
3. **SvelteKit frontend** (Vite on `:5173`): `just frontend`

Start in that order. The backend auto-connects to the decoder on `localhost:5403`. The frontend proxies `/api` and `/ws` to `localhost:3001` (configured in `frontend/vite.config.ts`).

To run backend without a decoder: `just server-no-decoder`

### Seeding demo data

`POST http://localhost:3001/api/seed-demo` creates a track, 8 riders, an event, and motos. Use this for quick manual testing. The endpoint is idempotent (skips if data exists).

### Database

SQLite via sqlx (`bmx-timing.db`), auto-created with migrations on server startup. No manual migration step needed.

### Gotchas

- The default Rust toolchain in the VM image may be too old (pre-1.85). The update script runs `rustup update stable` and `rustup default stable` to ensure edition 2024 support.
- `svelte-check` reports a11y warnings on the riders admin page — these are cosmetic, not errors.
