# Phase 4 Readiness Audit

Date: 2026-06-28.

Scope: phase 4 from
`/home/dean/chairman/docs/world-infra-unified-extraction-plan.md`.

Conclusion: Phase 4 is implemented for `world-event-core`, the narrow
delivery-facing slice of `notification-core`, and real `delivery-core` adapter
adoption in both products. `event-fanout` remains deferred. The shared crates
contain only product-neutral event envelope metadata, notification delivery
metadata, delivery worker contracts, and validators. Product SQL schemas,
provider clients, notification categories, and event payloads remain local.

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

Product adoption evidence:

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

Status: implemented for delivery metadata only.

Decision: create `notification-core`, but keep product notification policy out
of the shared crate.

Shared implementation evidence:

- `NotificationChannel` covers external delivery channels shared by both
  products: email, browser push, FCM, APNS, and webhook.
- `DeliveryVersion` validates positive product notification versions without
  defining product SQL tables or lifecycle rules.
- `NotificationTarget` validates generic external target strings while leaving
  channel-specific syntax and provider policy in products.
- `NotificationProviderOutcome` maps to `delivery-core::DeliveryAttemptOutcome`
  so provider adapters can share retry/finalization vocabulary.
- `NotificationAttempt` makes provider attempt metadata positive and 1-based at
  the type boundary.

Product canary evidence:

- Airline maps notification delivery rows, channels, delivery versions,
  positive provider attempts, and retry outcomes through shared types while
  preserving its notification center schema and delivery-version reopen
  semantics. Its notification worker uses `delivery-core` claim/finalize
  envelopes around product-owned repository calls.
- Chairman maps external alert delivery claims, channels, positive provider
  attempts, and provider outcomes through shared types while preserving its
  alert categories, urgency, payloads, and recipient policy. Its external alert
  worker uses `delivery-core` claim/finalize envelopes around product-owned
  SQL helpers for SMTP, webhook, browser push, FCM, and APNS modes.

Recorded local gates:

```sh
# world-infra
cargo fmt --check
cargo test -p notification-core
cargo clippy -p notification-core --all-targets -- -D warnings
cargo test --workspace --all-features
cargo test --workspace --no-default-features

# Chairman
cargo fmt --check
cargo test -p chairman-game-db external_alert_claim_maps_to_shared_notification_delivery_context
cargo test -p chairman-game-db external_alert_claim_rejects_invalid_shared_attempt
cargo test -p chairman-game-db cycle_completed_outbox_metadata_fits_world_event_envelope
cargo test -p chairman-worker alert_retry_backoff_is_bounded_and_terminal
cargo clippy -p chairman-worker -p chairman-game-db --all-targets -- -D warnings

# Airline
cargo fmt --check
cargo test -p loco-app work_item
cargo test -p loco-app delivery_state
cargo test -p loco-app cycle_completed_metadata_fits_world_event_envelope
cargo clippy -p loco-app --lib -- -D warnings
```

Still out of scope:

- notification categories;
- preferences and quiet hours;
- inbox/toast/read-state behavior;
- templates, copy, deep links, and managed actions;
- provider clients and product SQL schemas.

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

Phase 4 release-candidate evidence:

1. shared release source: `4fee1090d9b787b0e78b9af8bff156215beee885`;
2. next release tag: pending after product CI, expected
   `world-infra-v0.1.0-rc.12`;
3. Chairman delivery-core adoption commit:
   `4771761513b74c9c900c639210c00d08cfd8f014`;
4. Airline delivery-core/notification-core adoption commit:
   `b21f98cdd6905a30e162fa43c114bb7287168359`;
5. release review confirms `event-fanout` remains deferred and
   `notification-core` remains delivery-metadata-only.
