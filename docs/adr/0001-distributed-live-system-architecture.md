# ADR-0001: Distributed Live System Architecture for Multi-Track BMX Timing

- Status: Accepted
- Date: 2026-02-27
- Owners: Platform + Timing Team
- Tags: architecture, distributed-systems, realtime, jetstream, websocket

## Context

The system must evolve from a single-process/single-race runtime to a distributed, live architecture that can:

1. Ingest timing data from multiple physical tracks concurrently.
2. Power live dashboards with low-latency updates.
3. Support event management and race control workflows.
4. Tolerate transient network outages between track sites and central services.
5. Preserve event ordering per track and prevent duplicate processing.
6. Provide replay/debug capability without fragile coupling to in-memory state.

This repository already contains:

- `p3-track-client` (track-side decoder reader and HTTP batch sender)
- `p3-server` (central API/WebSocket/race engine)
- `frontend` (live displays and control UI)

The current architecture is suitable for development and local workflows but not for sustained concurrent multi-track live operation.

## Decision Drivers

- Per-track ordering guarantees for timing events.
- Idempotent processing under retries, reconnects, and at-least-once delivery.
- Bounded and predictable backpressure behavior.
- Horizontal scalability of ingest, processing, and fanout.
- Fast operator UX for race control and display.
- Simplicity for greenfield implementation (no migration constraints).

## Decision

Adopt an event-driven architecture centered on **NATS JetStream** as the durable transport between ingest and processing.

### Summary of the chosen target system

1. Track-side clients publish decoded timing events to a central ingest API.
2. Ingest API validates/authenticates, enriches metadata, and appends events to JetStream.
3. Stream processors consume events partitioned by `track_id`.
4. Race engines run per track (actor-style ownership), producing interpreted race events.
5. Projection workers persist query models for dashboards and event management.
6. WebSocket gateway serves filtered live streams by `track_id` and optional `event_id`.

## High-Level Architecture

### Control plane

- Event management API (tracks, riders, events, motos, race control).
- Stores canonical configuration and race control intents.

### Data plane

- Track ingest gateway and JetStream streams.
- Race processing workers and projection workers.
- Realtime fanout gateway for clients.

### Component view

1. **Track Gateway (`p3-track-client`, evolved)**
   - Reads local decoder stream.
   - Adds envelope metadata (`track_id`, `client_id`, `boot_id`, `seq`, timestamps).
   - Retries safely with idempotency key.
   - Optional local disk spool for WAN outages.

2. **Central Ingest API (`p3-server`, new `/api/ingest/batch`)**
   - Authenticates client.
   - Validates schema/contract version.
   - Publishes to JetStream (`timing.ingest.raw.v1`).
   - ACK only after durable append success.

3. **JetStream**
   - Durable event backbone.
   - Subject partitioning by `track_id`.
   - Replay support by sequence/time.

4. **Race Engine Workers (new worker role)**
   - Consumer group over `timing.ingest.raw.v1`.
   - One logical engine instance per `track_id`.
   - Emits derived events to `timing.race.events.v1`.

5. **Projection Workers**
   - Consume derived events.
   - Build read models for control UI, display UI, standings, diagnostics.
   - Persist to relational DB tables.

6. **WebSocket Gateway (`p3-server`, `/ws/v1/live`)**
   - Accepts scoped subscriptions.
   - Sends snapshots + incremental events.
   - Heartbeats and reconnect hints.

7. **Relational DB**
   - Stores control-plane entities and materialized read models.
   - No longer used as the primary ingest transport.

## Stream and Subject Design

Use versioned subjects to allow contract evolution.

- Raw ingest:
  - `timing.ingest.raw.v1.<track_id>`
- Derived race events:
  - `timing.race.events.v1.<track_id>`
- Snapshots:
  - `timing.race.snapshot.v1.<track_id>.<event_id>`
- Dead-letter:
  - `timing.dlq.v1.<source>`

### Why subject partitioning by `track_id`

- Maintains per-track ordering.
- Enables horizontal scaling by splitting track assignments across consumers.
- Avoids cross-track head-of-line blocking.

## Message Envelope and Idempotency

All raw ingest events use a canonical envelope:

- `event_id`: UUID/ULID
- `contract_version`: e.g. `track_ingest.v2`
- `track_id`
- `event_id_context`:
  - `client_id`
  - `boot_id` (generated at track client startup)
  - `seq` (monotonic per `boot_id`)
- `captured_at_us`
- `ingested_at_us`
- `message_type` (`PASSING`, `STATUS`, `VERSION`, ...)
- `payload` (decoded message body)

### Chosen idempotency key

`{track_id}:{client_id}:{boot_id}:{seq}`

Use this value as:

1. `Nats-Msg-Id` for JetStream duplicate suppression at publish time.
2. A persisted key in consumer-side dedupe logic for defense in depth.

### Delivery semantics

- Transport: at-least-once.
- Processing: effectively exactly-once via idempotent consumers.
- Ordering guarantee: per `track_id` only.

## Race Processing Model

Each track has an isolated race engine context:

- `track_id` -> `RaceEngineActor`
- Actor owns mutable race state for that track.
- Actor processes events sequentially from the track's subject.

Race control commands (`stage`, `reset`, `force-finish`) are persisted as intents and routed to the corresponding actor, then emitted as events for projections.

## WebSocket Subscription Contract

Endpoint:

- `GET /ws/v1/live?track_id=<required>&event_id=<optional>&channels=race,decoder&from=now`

Rules:

- `track_id` is required.
- `event_id` is optional.
- `channels` defaults to `race`.
- `from` defaults to `now` (future-compatible with replay markers).

Server envelope:

- `kind`: `snapshot | event | heartbeat | error`
- `channel`: `race | decoder`
- `track_id`
- `event_id` (nullable)
- `seq` (monotonic within channel scope)
- `ts_us`
- `payload`

Behavior:

- On connect: send latest snapshot for the subscription scope.
- Then stream incremental events.
- Send heartbeat every N seconds.
- On lag/failure: send error with reconnect strategy.

## Retention Policy

Retention is defined by **JetStream stream limits**.

Initial defaults (to tune after load testing):

1. `timing_ingest_raw_v1` stream
   - Subjects: `timing.ingest.raw.v1.*`
   - Retention: Limits
   - `MaxAge`: 7 days
   - `MaxBytes`: 100 GB
   - Discard policy: old
   - Rationale: high volume, short replay/debug window.

2. `timing_race_events_v1` stream
   - Subjects: `timing.race.events.v1.*`
   - Retention: Limits
   - `MaxAge`: 30 days
   - `MaxBytes`: 50 GB
   - Discard policy: old
   - Rationale: lower volume, longer operational replay needs.

3. `timing_race_snapshot_v1` stream
   - Subjects: `timing.race.snapshot.v1.*.*`
   - Retention: Limits
   - `MaxMsgsPerSubject`: 1
   - `MaxAge`: 24 hours
   - Rationale: latest-state bootstrap for websocket clients.

4. `timing_dlq_v1` stream
   - Subjects: `timing.dlq.v1.*`
   - Retention: Limits
   - `MaxAge`: 14 days
   - `MaxBytes`: 10 GB
   - Rationale: investigate poison messages without infinite growth.

### Retention sizing formula

Estimate daily storage:

`bytes_per_day ~= tracks * msgs_per_sec * avg_msg_bytes * 86400`

Set `MaxBytes` to at least:

`bytes_per_day * retention_days * 1.3`

where 1.3 is headroom for bursts and metadata overhead.

## Failure Handling and Reliability

1. Track gateway retries publishes with the same idempotency key.
2. If central ingest is unavailable, gateway stores to local spool and drains later.
3. Consumer ack deadlines and retry limits are configured per worker type.
4. Repeated failures route to DLQ with failure reason and original metadata.
5. Projection workers are fully idempotent and replay-safe.

## Security and Multi-Tenancy

- mTLS or signed token auth between track gateways and ingest API.
- Per-track authorization scope enforced at ingest and websocket layers.
- Audit log for control actions (`stage`, `reset`, `force-finish`).

## Observability and SLOs

Track core indicators:

- ingest append latency (p50/p95/p99)
- queue depth / consumer lag per track
- event processing lag (capture -> projection visible)
- websocket fanout delay
- duplicate suppression counts
- DLQ rate

Initial service objective:

- p95 end-to-end latency (captured event -> websocket client visible) < 500 ms under nominal load.

## Implementation Plan (Greenfield-Friendly)

Phase 1:

1. Add new ingest contract (`track_ingest.v2`) and `/api/ingest/batch`.
2. Add JetStream provisioning and publisher path.
3. Keep current dev ingest endpoints for local testing.

Phase 2:

1. Introduce race worker consuming `timing.ingest.raw.v1.*`.
2. Emit `timing.race.events.v1.*`.
3. Implement projection worker for race state/read models.

Phase 3:

1. Introduce `/ws/v1/live` subscription contract.
2. Wire frontend stores to scoped websocket subscriptions.
3. Add snapshot bootstrap stream.

Phase 4:

1. Remove direct local-decoder-to-engine coupling in central server runtime.
2. Make stream pipeline the sole source of truth for live data.
3. Harden SLO dashboards and alerting.

## Alternatives Considered

1. Keep direct HTTP ingest -> in-process race engine
   - Rejected: does not scale cleanly to multi-track concurrency and replay.

2. Use DB tables as queue
   - Rejected: introduces avoidable contention and operational complexity for ordered streaming.

3. Global websocket firehose
   - Rejected: poor tenant isolation, excess traffic, and scaling limits.

## Consequences

Positive:

- Clear separation of ingest, processing, and query/fanout concerns.
- Replayability and deterministic rebuild of projections.
- Per-track scalability with predictable ordering.

Trade-offs:

- More infrastructure and operational surface area.
- Requires strict event schema/version discipline.
- Requires idempotency in every consumer path.

## What This ADR Does Not Decide

- Exact database engine/version for long-term relational persistence.
- Final auth implementation detail (mTLS vs signed tokens) for gateways.
- Detailed UI behavior beyond websocket contract and subscription scope.

## Acceptance Criteria

The architecture is considered implemented when:

1. Track data from at least 3 concurrent tracks is processed without cross-track ordering violations.
2. Replaying a track's raw stream reproduces the same derived race outcomes.
3. Websocket clients can subscribe by track and receive only scoped events.
4. Ingest retries do not create duplicate race outcomes.
5. End-to-end latency SLO is met in representative load tests.

