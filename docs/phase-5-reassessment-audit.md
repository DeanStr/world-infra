# Phase 5 Reassessment Audit

Date: 2026-06-29.

Scope: Phase 5 from
`/home/dean/chairman/docs/world-infra-unified-extraction-plan.md`.

Conclusion: Phase 5 is complete as an incremental extraction phase, not a
blanket extraction phase. `world-test-containers`, `event-fanout`, and
`world-cycle-core` are implemented and adopted by both products where they fit.
Product databases, replay, authorization, Redis clients, WebSocket payloads,
SQL table names, migrations, and pipeline policy remain product-owned. Shared
SQLx crates, broader notification policy, idempotency backends, and broad DB
helpers remain deferred.

Implementation snapshot:

- Shared source: `8d582c230e42c87f3f8cc175b91bbeb83f5e4e37`.
- Airline canary: `ddbeb28ee5e0e4c24bc89a2e2c95b2e23b59a12c`.
- Chairman canary: `51a495fa13ce666ffe857805ef9f86c1722b018a`.
- A final tag and GitHub Actions evidence can be cut after local review; no
  additional code is required for Phase 5 completion.

## 1. world-cycle-core And world-cycle-sqlx

Decision: create `world-cycle-core`; defer `world-cycle-sqlx`.

Evidence:

- Airline has the mature control-plane model: `world_clock`, run/finalizing
  leases, `cycle_phase_state`, `sim_cycle_jobs`, stale-job audit, fault
  injection, and incarnation-scoped recovery. See
  `/home/dean/airline/docs/cycle-crash-recovery.md`,
  `/home/dean/airline/apps/loco-app/src/repo/world_clock/leases.rs`, and
  `/home/dean/airline/apps/loco-app/src/repo/world_clock/cycle_phase.rs`.
- Chairman has compatible concepts in design docs and some durable worker
  surfaces, but its implemented cycle authority is still product-shaped around
  the football daily-cycle pipeline. See
  `/home/dean/chairman/docs/chairman-game/15-worker-jobs-and-crash-recovery.md`
  and `/home/dean/chairman/crates/chairman-game-db/src/cycle.rs`.
- The shared crate must not freeze Airline's table names or Chairman's phase
  list. Phase 5 extracts only the product-neutral state-machine and lease
  primitives.

Completed shared implementation:

- `world-cycle-core` defines `WorkerId`, `LeaseState`, `LeasePolicy`,
  `CyclePhaseStatus`, `CyclePhaseClaim`, `FollowupRetryStatus`,
  `RetryBackoffPolicy`, `FollowupRetryPolicy`, lease-active helpers, stale
  cutoffs, and retention cutoffs.
- Airline uses shared phase claim/status mapping, lease helpers, stale cutoff,
  and retention cutoff while keeping SQL in `world_clock` and
  `cycle_phase_state`.
- Chairman maps shared `CyclePhaseStatus::Complete` to its product SQL label
  `"completed"` for `world_cycle_jobs` and `world_cycle_phase_state`.

Verification:

- `cargo test -p world-cycle-core`;
- `cargo clippy -p world-cycle-core --all-targets -- -D warnings`;
- Airline: `cargo check -p loco-app --lib`;
- Chairman: `cargo check -p chairman-game-db --all-targets`;
- See `docs/rfcs/world-cycle-followup-sqlx-phase-5.md`.

## 2. world-followup-sqlx

Decision: share follow-up retry vocabulary in `world-cycle-core`; defer
`world-followup-sqlx`.

Evidence:

- Airline has a durable `cycle_followup_retry_state` model with pending,
  running, complete, exhausted states, next-at scheduling, leases, retry
  dispatch, and prod-like enforcement. See
  `/home/dean/airline/apps/loco-app/src/jobs/followup_retry_state.rs` and
  `/home/dean/airline/apps/loco-app/src/jobs/followup_retry_enqueue.rs`.
- Chairman currently drains `outbox_events` and `external_alert_deliveries` with
  bounded batches and leased delivery rows, but does not yet have a separate
  generic cycle-followup retry table in code. See
  `/home/dean/chairman/crates/chairman-game-db/src/cycle.rs` and
  `/home/dean/chairman/apps/chairman-worker/src/delivery_utils.rs`.

Completed shared implementation:

- Airline uses shared `FollowupRetryStatus`, `FollowupRetryPolicy`, and
  zero-based retry backoff for `cycle_followup_retry_state`.
- Chairman already uses `delivery-core` for external alert delivery claim/finalize
  vocabulary; it does not need a generic cycle-follow-up SQL table yet.
- SQL ownership stays local. A future `world-followup-sqlx` would need Chairman
  to either adopt a generic follow-up table or map outbox/delivery rows into the
  same trait contract.

Verification:

- Airline: `cargo test -p loco-app followup_retry --lib`;
- Chairman: `cargo test -p chairman-game-db chairman_cycle_status_maps_shared_completion_to_product_label`.
- See `docs/rfcs/world-cycle-followup-sqlx-phase-5.md`.

## 3. event-fanout

Decision: create `event-fanout` as a narrow shared vocabulary crate.

Evidence:

- Airline has local broadcast plus Redis/Valkey pub/sub, source envelopes,
  ambiguous-after-attempt classification, durable `cycleCompleted` markers, and
  WebSocket per-connection dedupe. See
  `/home/dean/airline/apps/loco-app/src/events/fanout.rs`,
  `/home/dean/airline/apps/loco-app/src/events/cycle_broadcast.rs`, and
  `/home/dean/airline/docs/events/contracts.md`.
- Chairman currently serves world events by polling durable `outbox_events` over
  WebSocket. See
  `/home/dean/chairman/apps/chairman-api/src/routes/world_events_ws.rs` and
  `/home/dean/chairman/crates/chairman-game-db/src/read_models/events.rs`.
- Both products agree that product databases remain authoritative. The shared
  slice covers validated topics/node IDs, source identity, publish outcome
  classification, and ambiguous-after-attempt failure semantics only.

Completed shared implementation:

- `event-fanout` defines `FanoutTopic`, `FanoutNodeId`, `FanoutStableId`,
  `FanoutSource`, `FanoutFailureKind`, `FanoutFailure`, `PublishOutcome`, and
  `local_broadcast_outcome`.
- Redis/Valkey transport remains product-owned for now; Airline already has the
  concrete Redis runtime, while Chairman does not yet need cross-process fanout.
- Product-owned replay authority remains outside the crate.
- The adapter canaries are: Airline replaces local fanout failure-kind
  classification and local broadcast outcome classification; Chairman wraps
  WebSocket send attempts in `PublishOutcome` while retaining polling replay.

Verification:

- `cargo test -p event-fanout`;
- `cargo clippy -p event-fanout --all-targets -- -D warnings`;
- Airline focused event tests after pinning the product to the implementation
  revision;
- Chairman API world-event WebSocket tests after pinning the product to the
  implementation revision.

Deferred:

- Redis/Valkey shared transport primitives.
- In-process subscriber registries beyond simple send-result classification.
- Product event payloads, WebSocket wire shape, access control, durable markers,
  and replay queries.

- See `docs/rfcs/event-fanout-phase-5.md`.

## 4. Broader Notification Policy/Helpers

Decision: defer.

Evidence:

- Phase 4 deliberately kept `notification-core` narrow: external channels,
  delivery versions, targets, provider outcomes, and positive attempts.
- Airline's notification center, preferences, action-required semantics,
  delivery-version reopen behavior, and unsubscribe flows are product policy.
- Chairman's alert categories, urgency, account/world/club visibility, and
  football copy/deep links are product policy.

Do not extract yet:

- categories;
- preference or quiet-hour evaluation;
- notification center/read-state behavior;
- templates, copy, deep links, and managed actions;
- provider clients or product SQL schemas.

Allowed future shared work:

- narrowly typed provider error classification, only if both products map real
  provider adapters without importing product policy.

## 5. Idempotency Backends, DB Helpers, And Test Containers

Decision: split the lane.

Approved next small candidate: `world-test-containers`.

Reason:

- Airline already has reusable Testcontainers wrappers for Postgres and Valkey.
  See `/home/dean/airline/apps/loco-app/src/testing/postgres.rs` and
  `/home/dean/airline/apps/loco-app/src/testing/redis.rs`.
- Chairman has disposable-Postgres tests behind `CHAIRMAN_TEST_DATABASE_URL` but
  no equivalent shared container harness.
- A test-container crate can stay product-neutral by providing images,
  readiness waits, and optional features only. It should not run product
  migrations or seed product data.

Deferred:

- Redis/in-memory idempotency claim backends. Existing `idempotency-core`
  validates keys only; backend semantics are still product-owned and should wait
  for two real adapter examples.
- `world-db-sqlx`. Pool builders, migration runners, SQLSTATE helpers, and
  skip-locked helpers are tempting, but the current overlap is too broad. Pull
  out narrow helpers only after repeated code appears in both products with the
  same failure semantics.

Completed shared implementation:

- `world-test-containers` provides optional `postgres` and `valkey` features.
- The default feature set is empty, so normal shared-crate consumers do not pull
  Docker/Testcontainers dependencies.
- The crate exposes image wrappers and readiness waits only; migrations and seed
  data stay product-owned.

Product adoption:

- Shared crate verification:
  `cargo test -p world-test-containers --no-default-features`;
  `cargo test -p world-test-containers --features postgres`;
  `cargo test -p world-test-containers --features valkey`;
  `cargo test -p world-test-containers --all-features`;
  `cargo clippy -p world-test-containers --all-targets --all-features -- -D warnings`.
- Airline canary `80b60c4c1`:
  `cargo fmt --check`;
  `cargo check -p airline-utils --features tc`;
  `cargo check -p loco-app --features tc --lib`;
  `cargo test -p airline-utils --features tc`;
  `cargo check -p sim-engine --features tc --lib`.
- Chairman canary `3643c3b`:
  `cargo fmt --check`;
  `cargo test -p chairman-game-db shared_postgres_image_uses_chairman_test_defaults`;
  `cargo check -p chairman-game-db --all-targets`;
  `cargo test -p chairman-game-db`.

## Phase 5 Queue

1. Release hygiene: tag the final Phase 5 shared source and record GitHub
   Actions evidence.
2. Future phase: revisit `world-cycle-sqlx` and `world-followup-sqlx` only after
   both products intentionally converge on SQL adapter traits or table shapes.
