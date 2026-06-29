# RFC: world-cycle-sqlx and world-followup-sqlx Phase 5

## Status

Accepted with implementation split.

## Summary

Do not create shared SQLx crates yet. Airline has a mature cycle and follow-up
control plane, while Chairman has compatible concepts but a newer product-shaped
worker model. Phase 5 therefore extracts the shared non-SQL boundary into
`world-cycle-core`: lease arithmetic, phase/follow-up state vocabulary,
retention cutoffs, and retry policy. Product table names, queries, migrations,
and pipeline policy stay local.

## Product Evidence

- Airline adapter/example:
  `/home/dean/airline/apps/loco-app/src/repo/world_clock/leases.rs`,
  `/home/dean/airline/apps/loco-app/src/repo/world_clock/cycle_phase.rs`,
  `/home/dean/airline/apps/loco-app/src/jobs/followup_retry_state.rs`, and
  `/home/dean/airline/docs/cycle-crash-recovery.md` show mature leases,
  finalizing windows, retry follow-ups, incarnation checks, and stale recovery.
- Chairman adapter/example:
  `/home/dean/chairman/crates/chairman-game-db/src/cycle.rs`,
  `/home/dean/chairman/apps/chairman-worker/src/delivery_utils.rs`, and
  `/home/dean/chairman/docs/chairman-game/15-worker-jobs-and-crash-recovery.md`
  show cycle jobs, outbox draining, and leased external deliveries, but not a
  separate generic follow-up retry table.
- Approved deferral:
  Shared crates should wait until Chairman either maps its existing outbox and
  delivery rows into the same abstractions or explicitly adopts a small
  follow-up table. Broad SQL helpers stay out of scope.

## Boundaries

Product-owned:

- table names, migrations, and indexes;
- phase names and product pipeline order;
- job payloads and side effects;
- finalization policy and world reset/incarnation policy;
- observability labels that expose product concepts.

Shared in `world-cycle-core`:

- lease claim/release/expire calculations;
- validated status enums for pending, running, applied, complete, exhausted,
  and failed states;
- monotonic attempt and next-at calculations;
- stale lease reclaim policy inputs;

Still deferred:

- transaction-scoped SQL helper traits that operate on product-owned SQL;
- shared migrations or table names.

## API Sketch

```rust
pub struct LeasePolicy {
    pub owner: String,
    pub ttl: std::time::Duration,
    pub now_ms: i64,
}

pub enum PhaseProgress {
    Pending,
    Running,
    Applied,
    Complete,
    Failed,
}

pub trait CyclePhaseStore {
    type Error;

    async fn claim_phase(&mut self, key: &str, policy: LeasePolicy)
        -> Result<bool, Self::Error>;

    async fn mark_phase_applied(&mut self, key: &str) -> Result<(), Self::Error>;

    async fn mark_phase_complete(&mut self, key: &str) -> Result<(), Self::Error>;
}
```

The final API should prefer traits or query builders over shared migrations
unless both products intentionally adopt the same table shape.

## Compatibility

- Semver: new crates only after RFC acceptance.
- Persisted keys: product-owned; shared code may validate keys but should not
  define product key formats.
- Wire: none.
- SQL: high risk. Shared SQL must be transaction-scoped and either table-agnostic
  or backed by explicit shared migrations.
- Delivery: follow-up retries must not duplicate product side effects after
  ambiguous or partially-applied attempts.
- Rollback: products can keep existing cycle/follow-up code while adopting only
  pure policy helpers.

## Verification

Implemented verification:

- `world-cycle-core` tests cover phase claim mapping, active lease ownership,
  sub-second TTL preservation for Unix-second stores, follow-up terminal/delay
  semantics, zero-based capped retry backoff, attempt exhaustion, and retention
  cutoffs.
- Airline adapter canary uses shared phase claims, lease helpers, stale cutoff,
  follow-up retry status, retry exhaustion, and retry backoff against existing
  `world_clock`, `cycle_phase_state`, and `cycle_followup_retry_state` code.
- Chairman adapter canary maps shared cycle completion semantics to Chairman's
  product-owned `"completed"` SQL label in `world_cycle_jobs` and
  `world_cycle_phase_state`.
