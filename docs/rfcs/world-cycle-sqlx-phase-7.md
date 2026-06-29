# RFC: world-cycle-sqlx Phase 7 Trait Boundary

## Status

Proposed successor to `docs/rfcs/world-cycle-followup-sqlx-phase-5.md`.
Implementation is deferred.

## Summary

Do not create `world-cycle-sqlx` yet. Airline and Chairman now both use
`world-cycle-core` vocabulary, but their persistence boundaries are still
product-shaped. Phase 7 should define a transaction-bound adapter contract first
and require product characterization tests before any shared SQLx crate exists.

The likely shared surface is not query text. It is a small trait vocabulary for
claiming cycle work, recording phase progress, mapping shared phase statuses to
product labels, and proving stale recovery semantics while each product keeps
its own schema, enum labels, migrations, and authority checks.

## Product Evidence

| Product | Evidence | Current shape | Phase 7 implication |
| --- | --- | --- | --- |
| Airline | `/home/dean/airline/apps/loco-app/src/repo/world_clock/leases.rs` | Run leases require `world_id`, `world_instance_id`, owner id, target-cycle checks, ready-world checks, and heartbeat/release semantics. | Any adapter must carry incarnation identity and must not acquire connections or hide the product's authoritative world lookup. |
| Airline | `/home/dean/airline/apps/loco-app/src/repo/world_clock/cycle_phase.rs` | Phase claim and completion run inside `begin_world_tx`, set tenant scope, consult stale owner leases, and guard every mutation by `world_instance_id`. | A shared trait would need caller-owned transactions, explicit stale-cutoff inputs, and no product phase labels beyond shared vocabulary. |
| Airline | `/home/dean/airline/docs/cycle-crash-recovery.md` | Finalization is the authority boundary; phase states can be `running`, `applied`, or `complete`; stale owners are recovered only when the run lease is gone. | Shared code must preserve applied-without-replay and post-finalization follow-up boundaries. |
| Chairman | `/home/dean/chairman/crates/chairman-game-db/src/cycle.rs` | A scheduled or manual cycle is claimed and finalized inside the product's game-db pipeline; world clock and cycle job updates are coupled to football side effects. | A shared trait must not own the cycle transaction or move finalization policy out of `chairman-game-db`. |
| Chairman | `/home/dean/chairman/crates/chairman-game-db/src/rules/persistence/cycle_events.rs` | Shared `CyclePhaseStatus` maps to product enum labels, including `Complete -> "completed"`, and SQL binds are cast to product enum types. | Status mapping belongs in the product adapter unless both products intentionally adopt one persisted enum shape. |
| Chairman | `/home/dean/chairman/docs/chairman-game/15-worker-jobs-and-crash-recovery.md` | Chairman documents compatible leases, phase state, finalization, outbox, and repair concepts, but its implementation is intentionally newer and product-owned. | Phase 7 should add characterization tests before extraction, especially for repair, stale jobs, and outbox boundaries. |

## Phase 7 Decision

Defer implementation. Approve only the trait-boundary design lane now.

The current evidence supports a future adapter trait, but not shared SQL. Airline
has stronger incarnation and tenant-scope requirements than Chairman. Chairman
has product enum labels, product cycle jobs, and a football-specific
finalization pipeline. A shared SQLx crate today would either weaken Airline's
guards or force Chairman into Airline's mature control-plane shape.

## Boundaries

Product-owned:

- product table names, migrations, indexes, and enum labels;
- world lookup and authority checks;
- tenant scope and incarnation validation;
- cycle phase names and product pipeline order;
- finalization policy and side effects;
- repair tools, audit warnings, and operator messages;
- observability labels that expose product concepts.

Potentially shared after Phase 7 acceptance:

- trait names and associated types for cycle claims and phase stores;
- validated wrapper types for owner ids, target cycles, and phase names if both
  products can use them without changing persisted keys;
- conversion helpers between `world-cycle-core` statuses and adapter outcomes;
- test fixtures for lease/phase behavior that use fake in-memory adapters, not
  product SQL.

## API Sketch

Illustrative only. The final API should be smaller if product evidence allows.

```rust
pub trait CyclePhaseRepository {
    type Error;
    type Transaction<'tx>
    where
        Self: 'tx;
    type WorldIdentity;
    type Phase;
    type Owner;

    fn claim_phase<'tx>(
        tx: &'tx mut Self::Transaction<'tx>,
        world: Self::WorldIdentity,
        target_cycle: i64,
        phase: Self::Phase,
        owner: Self::Owner,
        stale_cutoff_unix_seconds: i64,
    ) -> impl Future<Output = Result<CyclePhaseClaim, Self::Error>> + Send + 'tx;

    fn mark_phase_applied<'tx>(
        tx: &'tx mut Self::Transaction<'tx>,
        world: Self::WorldIdentity,
        target_cycle: i64,
        phase: Self::Phase,
        owner: Self::Owner,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'tx;

    fn mark_phase_complete<'tx>(
        tx: &'tx mut Self::Transaction<'tx>,
        world: Self::WorldIdentity,
        target_cycle: i64,
        phase: Self::Phase,
        owner: Self::Owner,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'tx;
}
```

Contract constraints:

- The caller owns the transaction and commits or rolls it back.
- The shared trait must not accept a pool, start a transaction, or acquire a
  connection.
- The shared trait must not expose raw SQL identifiers or product enum labels.
- Product adapters must map shared vocabulary to product persistence values.
- Product adapters must prove whether incarnation is required, optional, or
  explicitly absent for each world identity.

## Compatibility

- Semver: no new public crate until the successor RFC is accepted.
- Persisted keys: product-owned; shared code may validate inputs but must not
  define product key formats.
- Wire: none.
- SQL: high risk. Any future SQLx crate must be transaction-bound, table-agnostic
  at the public API, and behind the narrowest possible `sqlx` feature surface.
- Delivery: cycle completion events and post-finalization effects remain
  product-owned.
- Rollback: products continue using local SQL and `world-cycle-core` helpers.

## Product Characterization Tests

Airline:

- `run_cycle_skips_stale_world_instance_job_before_mutation` proves stale
  world-instance cycle jobs do not mutate world state.
- `stale_world_instance_cannot_acquire_reused_world_run_lease` proves stale
  incarnations cannot acquire a reused numeric world's run lease.
- `cycle_phase_applied_state_skips_side_effect_and_can_complete_later` proves an
  `applied` phase can be completed without replaying the side effect.
- `cycle_phase_running_rows_are_busy_for_other_owners` and
  `cycle_phase_running_rows_can_be_reclaimed_after_owner_lease_is_stale` prove
  stale running phases are reclaimed only after the owning run lease is no longer
  live.
- `world_finalizing_requires_matching_run_lease_owner` and
  `complete_world_finalizing_is_owner_scoped_and_does_not_double_advance` prove
  finalization is owner-scoped and is the authority boundary for advancing the
  settled cycle.

Chairman:

- `chairman_cycle_status_maps_shared_completion_to_product_label` proves shared
  completion vocabulary still maps to Chairman's persisted `"completed"` label.
- `cycle_status_sql_binds_cast_text_to_product_enum` proves bound status values
  remain cast to the product enum type.
- `due_and_manual_cycle_claims_use_the_same_execution_target_shape` proves
  scheduled and manual cycle claims feed the same target/finalization path while
  preserving the product's final status.
- `phase_idempotency_keys_use_stable_phase_names` proves persisted phase
  idempotency keys remain product-stable.
- `scripts/chairman-backend-smoke.sh` assertions `admin cycle repair dry run`
  and `admin cycle repair apply` prove repair releases stale clock/job state
  without directly advancing football state.

## Verification Before Implementation

- RFC accepted with an explicit defer decision.
- Adapter sketches reviewed against both product repos.
- Product tests named above exist.
- `cargo test -p world-cycle-core` remains green.
- Product canaries prove behavior is unchanged before any future `world-cycle-sqlx`
  release candidate is tagged.
