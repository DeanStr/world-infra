# Phase 5 Reassessment Audit

Date: 2026-06-29.

Scope: Phase 5 from
`/home/dean/chairman/docs/world-infra-unified-extraction-plan.md`.

Conclusion: Phase 5 is started as a reassessment phase, not a blanket
extraction phase. The first shared-code candidate is `world-test-containers`.
`world-cycle-sqlx`, `world-followup-sqlx`, and `event-fanout` need design
records and product adapter examples before crate shells. Broader notification
policy and broad DB helpers remain deferred.

## 1. world-cycle-sqlx

Decision: design next, do not create the crate yet.

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

- RFC for SQL ownership: shared migrations vs product-owned tables.
- Two adapter sketches: Airline existing control tables and Chairman cycle jobs.
- Tests for stale lease reclaim, phase `applied` vs `complete`, finalization
  boundary, and world-incarnation mismatch.

## 2. world-followup-sqlx

Decision: promising but blocked on a second concrete implementation.

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

## 3. event-fanout

Decision: RFC required before code.

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

- RFC for Redis pub/sub vs streams vs NATS vs local broadcast.
- Explicit ambiguous-after-side-effect policy and marker semantics.
- Product-owned replay authority must remain outside the crate.

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

Required before product adoption:

- Product-neutral readiness tests that do not require product migrations.
- Airline canary replacing its local image wrappers.
- Chairman canary adding one disposable Postgres smoke without changing normal
  local-test requirements.

## Phase 5 Queue

1. Adopt `world-test-containers` in Airline first, then Chairman.
2. Draft `event-fanout` RFC using Airline as source material and Chairman as the
   polling-outbox counterexample.
3. Draft `world-cycle-sqlx` and `world-followup-sqlx` RFCs only after the
   test-container slice lands or a product urgently needs shared cycle SQL.
