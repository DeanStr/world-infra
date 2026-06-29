# Phase 7 Candidate Audit

Date: 2026-06-30

Scope: post-Phase 6 reassessment across `/home/dean/world-infra`,
`/home/dean/chairman`, and `/home/dean/airline`. Phase 7 starts as an
RFC/design lane. It should not add shared SQL or provider backends until both
products prove matching authority, transaction, retry, and recovery semantics.

Current baseline:

- Phase 6 release tag: `world-infra-v0.1.0-rc.15`
- Tagged crate source: `c7e55952da397856f945f232d8816c347c6dd9fb`
- Airline canary: `f3b52ecbe2e7d44283dd55674c212552acff52b7`
- Chairman canary: `6a60862b197416cfe52701d73332a272623693ce`

## Recommendation

Do not implement a new shared runtime crate as the first Phase 7 step. The best
next slice is to revise and split the accepted Phase 5 SQL deferral RFC into
Phase 7 successor RFCs that define adapter contracts and explicitly mark where
Airline and Chairman still diverge:

- `world-cycle-sqlx` trait-boundary RFC;
- `world-followup-sqlx` trait-boundary RFC.

The existing Phase 5 RFC is
`docs/rfcs/world-cycle-followup-sqlx-phase-5.md`. Phase 7 should treat that
document as the accepted deferral baseline, not as a competing source of truth.
The successor RFCs should supersede only the SQL deferral sections they
explicitly replace.

Phase 7 successor RFC drafts:

- `docs/rfcs/world-cycle-sqlx-phase-7.md`;
- `docs/rfcs/world-followup-sqlx-phase-7.md`.

Both candidates are useful, but both touch tables, transaction ownership,
leases, retry state, and product-specific recovery paths. Those are exactly the
places where Phase 5 and Phase 6 showed that vocabulary-first extraction works
better than premature shared SQL.

## Candidate Decisions

| Candidate | Decision | Reason | Phase 7 action |
| --- | --- | --- | --- |
| `world-cycle-sqlx` | RFC first | Airline cycle SQL is incarnation-aware and recovery-heavy; Chairman maps shared cycle statuses onto product enum labels and durable job/outbox rows. A shared query crate would freeze schema semantics too early. | Revise or split the Phase 5 RFC into a Phase 7 trait-boundary RFC covering claim, lease, stale recovery, completion, and phase status mapping without naming product tables. |
| `world-followup-sqlx` | RFC first | Airline owns a dedicated durable follow-up retry table; Chairman currently uses outbox and external-alert delivery rows. The shared shape is not proven twice yet. | Revise or split the Phase 5 RFC into a Phase 7 trait-boundary RFC for retry scheduling, lease ownership, attempt accounting, and terminal failure semantics. |
| Event-fanout transport backends | Defer implementation | Phase 5 added shared vocabulary only. Airline uses Redis/pubsub and durable cycle-completion markers; Chairman relies on durable outbox polling. | Keep transports local; revisit after both products share runtime signaling and replay requirements. |
| Redis/in-memory idempotency backends | Defer implementation | Airline's Redis idempotency is runtime-critical and incarnation-scoped; Chairman's idempotency is mostly SQL ledger/request handling. | Keep `idempotency-core` focused on key construction/validation unless both products adopt the same runtime claim semantics. |
| Notification provider semantics follow-up | Test/adoption only | Phase 6 intentionally stopped at sanitized provider outcome adapters. Airline now distinguishes retryable/permanent SMTP provider outcomes; Chairman keeps retry-until-exhaustion product-owned. | Add product tests where useful; do not move SMTP/WebPush/FCM/APNs clients or raw provider errors into world-infra. |
| Tiny SQLSTATE/error classification helper | Watchlist | Products may repeat small SQL error classification snippets, but no broad DB helper contract is proven. | Collect duplicates before extracting; prefer a tiny enum/helper over a broad `world-db-sqlx` crate. |
| `ledger-core` | Later | Ledger semantics remain domain-heavy: owner wealth, club finance, airline finance, and stable kind registries differ. | Reassess only for signed amount conventions or registry traits after both products expose matching ledger abstractions. |
| Auth/commercial-risk helpers | Separate lane | Security and commercial policy are product posture, not generic world infrastructure. | Keep outside Phase 7 unless both products explicitly approve a dedicated security/policy phase. |

## RFC Acceptance Gates

Before implementing `world-cycle-sqlx` or `world-followup-sqlx`, the RFC must
answer these questions for both products:

- Who owns the transaction boundary?
- Which product table or repository remains authoritative?
- How are world identity, world instance, incarnation, and tenant scope carried?
- What is the lease owner identity and expiration model?
- How are stale claims detected and recovered?
- Which statuses are shared vocabulary and which remain product enum labels?
- What does retryable, terminal, ambiguous-after-side-effect, and partial
  success mean for the adapter?
- Which product invariants must remain invisible to the shared crate?
- What focused product tests prove behavior is unchanged?
- What are the proposed trait signatures and adapter method shapes?
- Which product-owned repository calls those adapters?
- What are the explicit non-goals?

Each RFC must include concrete adapter sketches for Airline and Chairman. Those
sketches should show trait signatures, caller-owned transaction use, identity and
incarnation handling, lease semantics, retry and recovery semantics, and named
characterization tests. They should not include table names or query text yet.

## Future SQLx Constraints

If Phase 7 later approves a `*-sqlx` crate, the v0.1 implementation must obey
these constraints:

- No pool wrappers or hidden connection acquisition.
- No migrations, table names, product enum labels, or product-owned schema
  constants.
- No raw SQL identifier inputs.
- Transaction-bound APIs only; shared code must not start, commit, roll back, or
  hide transactions.
- The narrowest possible `sqlx` feature surface.
- Product adapters remain responsible for tenant scope, incarnation guarantees,
  and authority checks.

## Product Evidence To Gather

Airline:

- Cycle claim and stale-recovery flows in the route/campaign cycle stack;
- follow-up retry table lease semantics and attempt accounting;
- idempotency and event/fanout paths that require `world.instance_id` or
  incarnation safety;
- notification provider tests for permanent versus retryable SMTP and Web Push
  outcomes.

Chairman:

- Cycle job claim/completion and phase-state persistence in
  `chairman-game-db`;
- worker retry/exhaustion behavior for external alerts;
- outbox polling and WebSocket broadcast flows that currently stand in for
  durable event fanout transport;
- any SQLSTATE/error helpers repeated across API, worker, or game DB crates.

## Guardrails

- Do not extract SQL table names, enum labels, migrations, or query text until
  both products already share the same authority boundary.
- Do not weaken Airline's incarnation and tenant-scope guarantees.
- Do not force Chairman to adopt Airline's mature recovery tables just to use a
  shared crate.
- Do not let shared crates own provider clients, notification policy, recipient
  data, endpoints, tokens, raw provider bodies, or credential-bearing detail.
- Prefer trait/vocabulary contracts first; implement persistence only after two
  product adapters prove the same contract.

## Suggested Phase 7 Order

1. Revise or split the Phase 5 SQL deferral RFC into a `world-cycle-sqlx`
   Phase 7 trait-boundary RFC.
2. Revise or split the Phase 5 SQL deferral RFC into a `world-followup-sqlx`
   Phase 7 trait-boundary RFC.
3. Add characterization tests in products for any behavior the RFC names.
4. Reassess event-fanout transports and idempotency backends after the RFCs.
5. Implement only the smallest shared surface whose product adapters are already
   proven by tests.

## Completion Criteria

Phase 7's initial RFC/test-mapping slice is complete when:

- Successor RFCs record product evidence matrices for Airline and Chairman.
- Each RFC makes an explicit implement, defer, or reject decision.
- Transaction-boundary decisions are documented and mapped to adapter sketches.
- Named characterization tests exist for any product behavior the RFC relies on.
- Future implementation scope is small enough to avoid shared SQL policy,
  product schema ownership, and provider/runtime backend ownership.

For the current Phase 7 slice, both successor RFCs explicitly defer
implementation and name the product characterization tests that must keep passing
before either candidate can move beyond design.
