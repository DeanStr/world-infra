# RFC: world-followup-sqlx Phase 7 Trait Boundary

## Status

Proposed successor to `docs/rfcs/world-cycle-followup-sqlx-phase-5.md`.
Implementation is deferred.

## Summary

Do not create `world-followup-sqlx` yet. Airline and Chairman both have durable
post-cycle follow-up concepts, but they do not yet share one persistence model.
Airline owns a dedicated cycle follow-up retry table. Chairman uses durable
outbox events and external alert delivery rows, with provider-specific worker
draining layered on top.

Phase 7 should define the minimum adapter contract for retry scheduling, lease
ownership, attempt accounting, terminal failure, and ambiguous-after-side-effect
handling. The RFC should not move query text, product table names, provider
clients, or notification policy into world-infra.

## Product Evidence

| Product | Evidence | Current shape | Phase 7 implication |
| --- | --- | --- | --- |
| Airline | `/home/dean/airline/apps/loco-app/src/jobs/followup_retry_state.rs` | Follow-up rows are recorded with `world_id`, optional/resolved `world_instance_id`, cycle, step, status, attempts, next attempt time, terminal timestamps, and optional leases. Terminal rows are preserved. | A shared adapter must carry incarnation identity and preserve terminal-state monotonicity. |
| Airline | `/home/dean/airline/apps/loco-app/src/jobs/workers/cycle/followup.rs` | Follow-up side effects are marked running, completed, exhausted, or ambiguous; ambiguous marker failures intentionally prevent automatic duplicate side effects. | Shared vocabulary must represent ambiguity without forcing automatic replay. |
| Airline | `/home/dean/airline/docs/cycle-crash-recovery.md` | A hard stop after finalization leaves pending follow-up rows for job-enabled nodes to claim later. | Follow-up retries are downstream of finalization and must not be allowed to re-open the cycle authority boundary. |
| Chairman | `/home/dean/chairman/crates/chairman-game-db/src/cycle.rs` | Outbox events and external alert delivery rows are created from the cycle pipeline, then drained separately by workers. Alert delivery rows use leases and retry availability. | Chairman's follow-up model is split between product outbox and delivery rows; a single generic follow-up table is not proven. |
| Chairman | `/home/dean/chairman/apps/chairman-worker/src/delivery_utils.rs` | `delivery-core` and `notification-core` adapters classify claim/finalize outcomes, but retry exhaustion remains product policy. | Shared follow-up SQL must not change retry-until-exhaustion behavior or provider outcome policy. |
| Chairman | `/home/dean/chairman/docs/chairman-game/15-worker-jobs-and-crash-recovery.md` | Post-finalization work includes WebSocket fanout, notifications, analytics, delivery, cache invalidation, and audit events through durable outbox rows. | The RFC must distinguish follow-up scheduling from notification/provider delivery. |

## Phase 7 Decision

Defer implementation. Approve only the trait-boundary design lane now.

The current evidence supports shared terminology and tests, not a shared SQL
backend. Airline's table is a cycle-step retry state machine. Chairman's current
shape is an outbox plus delivery rows. A shared SQLx crate would either be too
abstract to help or would push one product's persistence model into the other.

## Boundaries

Product-owned:

- product table names, migrations, indexes, and enum labels;
- outbox event types, payloads, and fanout transports;
- notification policy, provider clients, recipient data, endpoints, tokens, and
  raw provider responses;
- partial-success semantics for multi-recipient or multi-device delivery;
- retry/exhaustion policy where it reflects product behavior;
- operator reconciliation and repair tools.

Potentially shared after Phase 7 acceptance:

- trait names and associated types for claim/finalize follow-up stores;
- validated step names or delivery ids if both products can adopt them;
- adapter outcomes for claimed, busy, terminal, retryable, exhausted, and
  ambiguous states;
- fake-adapter tests for lease expiry, attempt monotonicity, and terminal-state
  retention.

## API Sketch

Illustrative only. The final API should be smaller if product evidence allows.

```rust
pub trait FollowupRepository {
    type Error;
    type Transaction<'tx>
    where
        Self: 'tx;
    type WorldIdentity;
    type FollowupKey;
    type Owner;

    fn record_pending<'tx>(
        tx: &'tx mut Self::Transaction<'tx>,
        world: Self::WorldIdentity,
        key: Self::FollowupKey,
        attempt: u32,
        next_attempt_unix_seconds: i64,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'tx;

    fn claim_due<'tx>(
        tx: &'tx mut Self::Transaction<'tx>,
        owner: Self::Owner,
        lease_expires_unix_seconds: i64,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<Self::FollowupKey>, Self::Error>> + Send + 'tx;

    fn finalize<'tx>(
        tx: &'tx mut Self::Transaction<'tx>,
        key: Self::FollowupKey,
        outcome: FollowupFinalizeOutcome,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'tx;
}
```

Contract constraints:

- The caller owns the transaction and commits or rolls it back.
- The shared trait must not accept a pool, start a transaction, or acquire a
  connection.
- The shared trait must not include provider clients or raw provider error data.
- Terminal states must be monotonic unless the product adapter explicitly defines
  operator repair behavior.
- Ambiguous-after-side-effect must never imply automatic duplicate replay.

## Compatibility

- Semver: no new public crate until the successor RFC is accepted.
- Persisted keys: product-owned; shared code may validate inputs but must not
  define product key formats.
- Wire: none.
- SQL: high risk. Any future SQLx crate must be transaction-bound, table-agnostic
  at the public API, and behind the narrowest possible `sqlx` feature surface.
- Delivery: provider outcomes remain in `notification-core`; provider delivery
  policy remains product-owned.
- Rollback: products continue using local SQL and `world-cycle-core`,
  `delivery-core`, and `notification-core` helpers.

## Product Characterization Tests

Airline:

- `claim_due_cycle_followups_leases_rows_until_the_lease_expires` proves due
  follow-up claims respect lease ownership and lease expiry.
- `delayed_followup_rows_are_not_claimed_until_due` proves retry delay is honored
  before claiming.
- `claimed_retry_executes_known_steps_and_marks_them_complete` proves known
  follow-up steps reach a terminal complete state.
- `claimed_retry_exhausts_unknown_steps_without_requeueing` proves terminal
  exhausted follow-ups stop retrying.
- `completion_marker_failure_after_side_effect_is_listed_for_manual_reconciliation`
  proves ambiguous follow-up marker failures do not automatically duplicate side
  effects.

Chairman:

- `cycle_completed_outbox_metadata_fits_world_event_envelope` proves cycle
  completion is recorded through durable outbox metadata, separate from provider
  delivery.
- `external_alert_claim_maps_to_shared_notification_delivery_context` proves
  claimed external alert rows map into shared notification delivery context
  without moving provider policy into world-infra.
- `alert_retry_backoff_is_bounded_and_terminal` proves retryable failures without
  provider hints requeue with Chairman's default delay and eventually become
  terminal.
- `alert_provider_outcome_adapter_does_not_change_exhaustion_policy` proves retry
  exhaustion remains controlled by Chairman's max-attempt policy, not provider
  classification alone.
- `scripts/chairman-backend-smoke.sh` assertions `admin cycle repair dry run`,
  `admin cycle repair apply`, and worker external-alert drain checks remain the
  DB-backed coverage for outbox and alert delivery recovery.

## Verification Before Implementation

- RFC accepted with an explicit defer decision.
- Adapter sketches reviewed against both product repos.
- Product tests named above exist.
- `cargo test -p world-cycle-core -p delivery-core -p notification-core` remains
  green.
- Product canaries prove behavior is unchanged before any future
  `world-followup-sqlx` release candidate is tagged.
