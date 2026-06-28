# Phase 4 Readiness Audit

Date: 2026-06-28.

Scope: phase 4 from
`/home/dean/chairman/docs/world-infra-unified-extraction-plan.md`.

Conclusion: Phase 4 is implemented for `world-event-core` and explicitly
deferred for `notification-core` and `event-fanout`. The shared crate contains
only product-neutral event envelope metadata and validators. Chairman and
Airline both have focused consumer canaries against exact `world-infra`
revision `41049ae237fd5367a23494d2cdb3f9ef93895bfe`.

## world-event-core

Status: implemented and locally canaried in both products.

Shared implementation evidence:

- `world-event-core` defines validated metadata newtypes for event IDs, event
  types, schema versions, aggregate IDs, sources, causation IDs, and correlation
  IDs.
- `EventMetadata<W, I>` carries `WorldRef<W, I>` from `world-identity-core`,
  optional `idempotency-core::IdempotencyKey`, produced-at time, source, and a
  neutral durability boundary.
- `EventEnvelope<W, I, P>` carries the product-owned payload without defining a
  product event schema, database shape, transport topic, or serialized wire
  format.
- Generic envelope types intentionally do not derive serde. Products own explicit
  wire adapters.
- Product-neutral tests cover metadata validation, no-incarnation envelopes, and
  incarnation-aware envelopes.

Product canary evidence:

- Chairman `chairman-game-db` constructs a
  `WorldRef<Uuid, NoIncarnation>` envelope from existing
  `world.cycle_completed` outbox metadata without changing SQL inserts, outbox
  reads, WebSocket JSON, or runtime dependencies.
- Airline `loco-app` constructs a
  `WorldRef<i32, IncarnationId<Uuid>>` envelope from the existing
  `cycleCompleted` event ID and payload, preserving `world_instance_id`, durable
  dedupe IDs, and current WebSocket JSON.

Recorded local gates:

```sh
# world-infra
cargo fmt --check
cargo test -p world-event-core
cargo clippy -p world-event-core --all-targets -- -D warnings

# Chairman
cargo test -p chairman-game-db cycle_completed_outbox_metadata_fits_world_event_envelope

# Airline
cargo test -p loco-app cycle_completed_metadata_fits_world_event_envelope
```

## notification-core

Status: deferred.

Decision: do not create `notification-core` in Phase 4.

Reason:

- `delivery-core` now captures the common retry, lease, attempt-outcome, and run
  report vocabulary.
- Chairman and Airline still differ materially in notification categories,
  preference filtering, quiet hours, delivery versions, managed actions,
  provider workflows, inbox/toast semantics, deep links, and user-facing copy.
- Extracting those pieces now would either overfit Airline's mature notification
  center or prematurely constrain Chairman's external alert model.

Required future gate:

- separate RFC;
- both-product adapter examples or a dated approved deferral;
- rollback notes for delivery semantics;
- product canaries proving no notification wire, queue, SQL, or user-copy drift.

## event-fanout

Status: deferred.

Decision: do not create `event-fanout` in Phase 4.

Reason:

- Chairman currently uses durable Postgres outbox replay over WebSocket.
- Airline has Redis fanout, local broadcast, cycle-info boundary markers,
  durable publish-accepted markers, and ambiguity reconciliation.
- The shared event envelope is useful now, but fanout failure semantics are not
  yet product-neutral.

Required future gate:

- stable `world-event-core` adoption in both products;
- separate fanout RFC;
- explicit treatment of Redis/Valkey/NATS topic naming, replay guarantees,
  ambiguous-after-side-effect outcomes, and product-owned authoritative state.

## Release Readiness

Phase 4 is ready for a release-candidate tag after:

1. shared CI passes for revision `41049ae237fd5367a23494d2cdb3f9ef93895bfe`;
2. Chairman and Airline exact-revision product commits pass CI;
3. release review confirms `notification-core` and `event-fanout` remain
   deferred.
