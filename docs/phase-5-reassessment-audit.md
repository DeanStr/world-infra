# Phase 5 Reassessment Audit

Date: 2026-06-29.

Scope: Phase 5 from
`/home/dean/chairman/docs/world-infra-unified-extraction-plan.md`.

Conclusion: Phase 5 is an incremental extraction phase, not a blanket
extraction phase. The first shared-code candidate, `world-test-containers`, is
implemented and adopted by both products. `event-fanout`, `world-cycle-sqlx`,
and `world-followup-sqlx` now have design records, but they still need product
adapter examples before crate shells. Broader notification policy and broad DB
helpers remain deferred.

Release candidate:

- Shared source: `world-infra-v0.1.0-rc.13`, pointing at
  `a366a855ac35a4e02a281a3d1c43761ca90aefc0`.
- Airline canary: `80b60c4c1` pins that revision, replaces local Postgres and
  Valkey image definitions with `world-test-containers`, and preserves existing
  test helper exports.
- Chairman canary: `3643c3b` pins that revision, adds a normal no-Docker image
  defaults test, and adds an ignored disposable Postgres smoke.

## 1. world-cycle-sqlx

Decision: RFC drafted, do not create the crate yet.

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
  list. It needs product-owned table adapters or a minimal schema contract first.

Required before implementation:

- Two adapter implementations or executable sketches: Airline existing control
  tables and Chairman cycle jobs.
- Tests for stale lease reclaim, phase `applied` vs `complete`, finalization
  boundary, and world-incarnation mismatch.
- See `docs/rfcs/world-cycle-followup-sqlx-phase-5.md`.

## 2. world-followup-sqlx

Decision: RFC drafted, still blocked on a second concrete implementation.

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

Required before implementation:

- Chairman adapter or explicit approved deferral showing how existing outbox and
  external delivery rows map to follow-up status, attempts, next-at, lease owner,
  and terminal states.
- Decision on whether the crate owns SQL tables or only query builders/traits.
- Product canaries proving finalization-after-followup failure does not duplicate
  WebSocket, notification, analytics, or delivery side effects.
- See `docs/rfcs/world-cycle-followup-sqlx-phase-5.md`.

## 3. event-fanout

Decision: RFC drafted before code.

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
- Both products agree that product databases remain authoritative, but they do
  not yet share transport semantics or replay guarantees.

Required before implementation:

- Explicit ambiguous-after-side-effect policy and marker semantics.
- Product-owned replay authority must remain outside the crate.
- Airline adapter canary and Chairman adapter canary.
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
  readiness waits, connection URL helpers, and optional features only. It should
  not run product migrations or seed product data.

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

1. Monitor Airline and Chairman CI for the `world-test-containers` canaries.
2. Use the `event-fanout` RFC to build the next thin shared slice, with Airline
   as source material and Chairman as the
   polling-outbox counterexample.
3. Turn the cycle/followup RFC into product adapter sketches before creating any
   SQLx crate.
