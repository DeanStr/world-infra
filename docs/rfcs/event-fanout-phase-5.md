# RFC: event-fanout Phase 5

## Status

Accepted and implemented as a thin Phase 5 slice.

## Summary

Create a product-neutral event fanout crate only after both products agree on a
small transport contract. The shared code should coordinate best-effort
real-time delivery across local process subscribers and optional Redis/Valkey
pub/sub. Product databases remain authoritative for replay, retention, access
control, event shape, and durable markers.

The first shared slice should be transport glue and delivery classification, not
a product event store.

## Product Evidence

- Airline adapter/example:
  `/home/dean/airline/apps/loco-app/src/events/fanout.rs`,
  `/home/dean/airline/apps/loco-app/src/events/cycle_broadcast.rs`, and
  `/home/dean/airline/docs/events/contracts.md` show local broadcast plus
  Redis/Valkey pub/sub, source envelopes, durable `cycleCompleted` markers,
  ambiguous-after-attempt handling, and WebSocket dedupe.
- Chairman adapter/example:
  `/home/dean/chairman/apps/chairman-api/src/routes/world_events_ws.rs` and
  `/home/dean/chairman/crates/chairman-game-db/src/read_models/events.rs` show
  a polling WebSocket model over durable `outbox_events`.
- Approved deferral:
  Do not extract product replay queries, event DTOs, WebSocket authorization, or
  database schemas. Chairman does not need to adopt Redis fanout before the
  shared crate can exist, but it needs an adapter canary proving the polling
  path can share classification and envelope logic.

## Boundaries

Product-owned:

- event schemas, categories, and payload compatibility;
- durable outbox/event tables and replay queries;
- WebSocket authentication, visibility filtering, and subscription scoping;
- marker persistence, if a product stores markers in product tables;
- choice to enable Redis/Valkey, local-only, or polling-only delivery.

Shared:

- typed fanout publish outcomes, including delivered, no subscribers, and
  ambiguous-after-side-effect;
- source identity for local, remote, replay, and self-originated messages;
- optional Redis/Valkey pub/sub transport primitives;
- in-process subscriber registry primitives;
- key/channel validation compatible with `rate-limit-core` key discipline.

## API Sketch

```rust
pub enum FanoutSource {
    Local,
    Remote { node_id: String },
    Replay,
}

pub enum PublishOutcome {
    Published,
    NoSubscribers,
    AmbiguousAfterAttempt { error: String },
}

pub trait FanoutEnvelope {
    fn topic(&self) -> &str;
    fn stable_id(&self) -> &str;
    fn source(&self) -> FanoutSource;
}

pub trait LocalSubscriber<E> {
    fn try_send(&self, envelope: E) -> PublishOutcome;
}
```

The real API should avoid owning async runtime policy. Redis/Valkey support
should be feature-gated and adapter-based.

## Compatibility

- Semver: new crate, additive.
- Persisted keys: no product database keys in the first slice; Redis/Valkey
  channels must use validated namespaces/topics.
- Wire: no product event payloads. If the crate serializes transport envelopes,
  the envelope version must be explicit.
- SQL: none in the first slice.
- Delivery: best effort only. Products still own replay and exactly-once or
  effectively-once semantics through their databases.
- Rollback: products can keep local fanout implementations and ignore the crate.

## Verification

Shared tests:

- local broadcast classification reports active subscribers and treats closed
  local broadcast channels as `NoSubscribers`;
- ambiguous-after-attempt is preserved instead of downgraded to failure;
- invalid channel/topic names are rejected;
- Redis/Valkey transport remains product-owned in this slice.

Consumer canaries:

- Airline replaces local outcome/source classification while retaining current
  Redis pub/sub and WebSocket dedupe tests.
- Chairman wraps polling WebSocket sends in the shared outcome type without
  changing durable replay authority.
