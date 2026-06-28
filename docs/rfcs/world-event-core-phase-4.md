# RFC: world-event-core Event Envelopes

## Status

Proposed

## Summary

Define a small, product-neutral event-envelope crate for persistent
multiplayer-world products. The crate should describe stable event metadata and
identity boundaries, while leaving event payloads, product event names, SQL
tables, fanout transports, scheduler policy, and notification semantics in the
product repositories.

This RFC starts Phase 4 as an inventory and API boundary. It does not approve a
crate shell yet. Implementation should wait until both products approve the
envelope shape and name the first adapter canaries.

## Product Evidence

- Chairman adapter/example:
  - Chairman stores durable world events in `outbox_events` with `id`,
    `world_id`, `event_type`, `aggregate_type`, `aggregate_id`,
    `idempotency_key`, `payload`, `created_at`, `available_at`, and
    `delivered_at`.
  - `chairman-game-db` inserts events such as `world.cycle_completed`,
    `world.season_completed`, and ownership conflict events inside the same
    transaction as authoritative cycle or ownership state.
  - The API replays visible events over `world_events_ws`, using the database
    UUID event ID as the cursor and product-owned visibility rules for private
    account events.
  - Chairman currently has no world incarnation dimension, so an adapter would
    use `WorldRef<Uuid, NoIncarnation>`.
- Airline adapter/example:
  - Airline has an incarnation-aware durable cycle event ID:
    `cycle-completed:{world_id}:{world_instance_id}:{cycle}`.
  - `cycle_event_broadcast` records reservation, cycle-info boundary,
    publish-attempt ambiguity, sent state, and retry reconciliation for
    `cycleCompleted` fanout.
  - Client WebSocket contracts define `cycleCompleted.eventId` as the durable
    dedupe key and include `worldInstanceId` for new events.
  - `sim-engine` may emit non-authoritative telemetry, while `loco-app` owns
    durable `cycleCompleted` emission after the finalization boundary commits.
  - An adapter would use `WorldRef<i32, IncarnationId<Uuid>>`.
- Approved deferral, if any:
  - `notification-core` remains deferred. `delivery-core` is the only approved
    shared delivery surface for now. Notification categories, preference
    policy, delivery versions, quiet hours, managed actions, user copy, and UI
    semantics remain product-owned.
  - `event-fanout` remains deferred until event envelopes are stable and both
    products agree which Redis/Valkey or local broadcast failure semantics are
    truly common.

## Boundaries

Shared by `world-event-core`:

- validated event metadata newtypes for event ID, event type, schema version,
  aggregate ID, event source, causation ID, and correlation ID;
- an envelope type that carries `WorldRef<W, I>` from `world-identity-core`;
- an explicit payload type parameter;
- neutral durability-boundary labels that adapters can map to product-owned
  outbox, commit, or fanout states;
- helper constructors that preserve idempotency keys without inventing product
  key formats.

Product-owned:

- event payload structs and JSON schema contracts;
- event type taxonomy and naming conventions;
- cycle phases, scheduler policy, finalization policy, and simulation outputs;
- SQL migrations, table names, leases, retention, replay queries, and RLS;
- Redis/Valkey/NATS/WebSocket topic names and transport-specific fanout rules;
- notification categories, preference filtering, delivery versions, managed
  actions, deep links, inbox/toast semantics, and user-facing copy.

## API Sketch

```rust
use std::time::SystemTime;

use idempotency_core::IdempotencyKey;
use world_identity_core::WorldRef;

pub struct EventEnvelope<W, I, P> {
    pub event_id: EventId,
    pub event_type: EventType,
    pub schema_version: SchemaVersion,
    pub world: WorldRef<W, I>,
    pub aggregate_id: Option<AggregateId>,
    pub idempotency_key: Option<IdempotencyKey>,
    pub produced_at: SystemTime,
    pub source: EventSource,
    pub causation_id: Option<EventId>,
    pub correlation_id: Option<CorrelationId>,
    pub durability: DurabilityBoundary,
    pub payload: P,
}

pub enum DurabilityBoundary {
    EphemeralSignal,
    AuthoritativeStateCommitted,
    DurableOutboxRecorded,
    PublishAccepted,
    PublishAttemptAmbiguous,
}

pub struct EventId(String);
pub struct EventType(String);
pub struct SchemaVersion(u32);
pub struct AggregateId(String);
pub struct EventSource(String);
pub struct CorrelationId(String);
```

The exact names are illustrative. The implementation should avoid deriving
generic `serde` for `EventEnvelope<W, I, P>` by default. Products should expose
explicit wire adapters, matching the `world-identity-core` pattern, so shared
generic types do not accidentally become public API contracts.

## Compatibility

- This RFC changes no code, persisted data, wire format, Redis key, SQL schema,
  or product behavior.
- A future v0.1 crate must be additive and feature-light. It may depend on
  `world-identity-core` and, if both products approve, `idempotency-core`.
- `EventEnvelope<W, I, P>` should not commit a universal serialized shape.
  Product adapters own conversion to Chairman's API event reports and Airline's
  WebSocket/Redis message contracts.
- Durability labels must be descriptive metadata only. They must not force a
  product to change when it records an outbox row, marks publish acceptance, or
  treats a fanout result as ambiguous.
- Existing event IDs are product-owned. The shared crate may validate strings,
  but it must not parse Chairman or Airline event ID formats as universal
  structure.
- If a future release adds `event-fanout` or `notification-core`, it needs a
  separate RFC with both-product evidence and rollback notes.

## Verification

Before crate implementation:

- add product-neutral tests for each metadata validator;
- add construction tests for no-incarnation and incarnation-aware
  `WorldRef<W, I>` envelopes;
- prove generic envelope types have no accidental default wire shape;
- document feature combinations and semver expectations.

Consumer canaries required before release:

- Chairman: adapt one existing outbox insert/read fixture to construct an
  envelope with `WorldRef<Uuid, NoIncarnation>` without changing the database or
  WebSocket JSON.
- Airline: adapt one `cycleCompleted` fixture to construct an envelope with
  `WorldRef<i32, IncarnationId<Uuid>>`, preserving stale-world-instance checks,
  durable dedupe IDs, and current WebSocket JSON.
- Both products: run focused event/outbox/WebSocket tests and record the exact
  `world-infra` revision in release evidence before tagging.
