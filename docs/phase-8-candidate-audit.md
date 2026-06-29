# Phase 8 Candidate Audit

Date: 2026-06-30

Scope: post-Phase 7 reassessment across `/home/dean/world-infra`,
`/home/dean/chairman`, and `/home/dean/airline`.

Current baseline:

- Phase 7 release tag: `world-infra-v0.1.0-rc.16`
- Tagged source: `e14079153a0c84043087e9d2fa3be6afcce70186`
- Airline canary: `4cf108b5bc276c1d9ce7bb0ca9e416af7915cd88`
- Chairman canary: `7156d695857a951d807ad7dc2fd2dcb430979258`

## Recommendation

Phase 8 should be a product-first hardening phase, not a shared-crate sprint.
The strongest opportunity is to improve Chairman's newer worker, recovery,
outbox, and delivery flows using lessons from Airline's mature systems, then
extract only the small surfaces that are proven twice.

This is intentionally different from Phase 7. Phase 7 wrote trait-boundary RFCs
and deferred shared SQL. Phase 8 should make one product better first, while
recording which improvements become repeated enough to deserve `world-infra`
surface area later.

## Candidate Decisions

| Candidate | Decision | Reason | Phase 8 action |
| --- | --- | --- | --- |
| Chairman cycle/follow-up recovery hardening | Implement in Chairman first | Airline has more mature stale-claim, retry, reconciliation, and lease-recovery patterns. Chairman should benefit from those ideas without forcing its schema into a shared SQL crate. | Add product-local tests and code for the highest-value recovery gaps: stale job reclaim, repair visibility, outbox/drain safety, and retry exhaustion clarity. |
| Shared `world-cycle-sqlx` / `world-followup-sqlx` crates | Continue to defer | Phase 7 proved the design vocabulary but still did not prove identical authority, transaction, table, or recovery semantics. | Revisit only after Chairman hardening produces product-owned adapters matching the Phase 7 RFC sketches. |
| Event-fanout transport backend | Defer runtime extraction | Airline and Chairman still differ on Redis/pubsub versus durable outbox polling and replay expectations. | Characterize product signaling requirements; extract no transport until both products share delivery, replay, and failure semantics. |
| Idempotency runtime backend | Defer runtime extraction | Airline's runtime idempotency is Redis/incarnation-heavy; Chairman is more SQL/request-led. | Keep `idempotency-core` as key vocabulary unless both products adopt a matching runtime claim contract. |
| Tiny SQLSTATE/error classification helper | Watchlist | Both products may repeat narrow database error classification, but a broad DB helper remains too risky. | Collect duplicates while doing product work; extract only a tiny enum/helper if repetition becomes obvious. |
| Ledger conventions | Audit only | Finance and ledger authority are still product-domain heavy. | Audit amount-sign, ledger-kind, and registry patterns. Do not add `ledger-core` unless both products expose a shared convention independent of game/business policy. |
| Auth/commercial-risk helpers | Out of scope | These are product security and business posture, not generic world infrastructure. | Keep outside Phase 8 unless a dedicated security/policy phase is explicitly approved. |

## Product-First Work

Chairman is the main Phase 8 beneficiary:

- compare Chairman cycle job repair and stale-recovery paths against Airline's
  mature cycle/follow-up behavior;
- add characterization tests before changing worker behavior;
- keep Chairman schema and enum labels product-owned;
- improve observability for stuck jobs, retry exhaustion, and repair decisions;
- preserve current outbox and external-alert delivery semantics unless a test
  proves a deliberate behavior change.

Airline should mostly stay stable during this phase:

- use Airline as evidence and comparison material, not as a target for broad
  rewrites;
- add small characterization tests only where they clarify a future shared
  contract;
- avoid changing mature recovery behavior just to make Chairman look similar.

## Extraction Gates

Before adding any Phase 8 shared crate or module, require:

- a product-local Chairman improvement with tests already merged;
- matching Airline evidence showing the same contract exists there too;
- a small API that does not name product tables, product enum labels, migrations,
  pool wrappers, or provider clients;
- caller-owned transactions or executors for any SQL-adjacent design;
- no weakening of Airline's incarnation, tenant-scope, or idempotency guarantees;
- a migration-free adoption path for both products.

## Suggested Phase 8 Order

1. Audit Chairman worker/cycle/follow-up recovery against Airline's mature paths.
2. Add Chairman characterization tests for the highest-risk recovery gaps.
3. Implement product-local Chairman hardening where the tests expose a real gap.
4. Record any duplicated, product-neutral helper candidates discovered while
   hardening Chairman.
5. Reassess whether a tiny shared helper is justified. Prefer no extraction if
   the only shared shape is still aspirational.

## Completion Criteria

Phase 8 is complete when:

- Chairman has product-local recovery/worker hardening evidence or an explicit
  no-change rationale for each audited gap.
- Airline evidence is recorded for any comparison used to justify a future
  shared contract.
- Any extracted `world-infra` surface is tiny, product-neutral, and proven by
  both products before adoption.
- Deferred candidates remain documented with concrete reasons rather than vague
  "later" language.
- CI evidence is recorded for `world-infra`, Chairman, and Airline if any repo
  changes during the phase.
