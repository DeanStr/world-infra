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

## Decision

Defer implementation. Write only the trait-boundary contract now.

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

## Required Product Characterization Tests

Airline:

- Claiming a phase for a stale world incarnation is rejected.
- A stale running phase is reclaimed only after the owning run lease is gone.
- An `applied` phase can be completed without replaying its side effect.
- Finalization remains the only authority boundary for advancing the settled
  cycle.

Chairman:

- Shared completion status still maps to the product's persisted completion
  label.
- Bound status values remain cast to the product enum type.
- Manual and scheduled cycle claims use the same product finalization path.
- Cycle repair releases stale clock/job state without directly advancing football
  state.

## Verification Before Implementation

- RFC accepted with an explicit implement/defer decision.
- Adapter sketches reviewed against both product repos.
- Product tests named above exist or are mapped to existing test names.
- `cargo test -p world-cycle-core` remains green.
- Product canaries prove behavior is unchanged before any future `world-cycle-sqlx`
  release candidate is tagged.
