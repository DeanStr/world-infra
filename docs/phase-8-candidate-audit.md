# Phase 8 Candidate Audit

Date: 2026-06-30

Scope: post-Phase 7 reassessment across `/home/dean/world-infra`,
`/home/dean/chairman`, and `/home/dean/airline`.

Current baseline:

- Phase 7 release tag: `world-infra-v0.1.0-rc.16`
- Tagged source: `e14079153a0c84043087e9d2fa3be6afcce70186`
- Airline canary: `4cf108b5bc276c1d9ce7bb0ca9e416af7915cd88`
- Chairman canary: `7156d695857a951d807ad7dc2fd2dcb430979258`

`world-infra-v0.1.0-rc.16` has no shared crate-code delta over the Phase 6
crate source consumed by products. It records Phase 7 RFC/test-mapping evidence
only, so the product evidence above remains the `world-infra-v0.1.0-rc.15`
consumer canaries.

Post-`rc.16` maintenance fixes:

- `815e3bff27ffcab4b8813594b41467f8a8b77dac` fixes fractional
  rate-limit windows and local release packaging patches.
- `53a232e541640122f66effd03058000787379a4e` exposes default
  Testcontainers service ports.

These are correctness and release-hygiene fixes to existing shared crates, not
Phase 8 extraction work. They should be covered by a follow-up maintenance
release candidate before any product pin refresh.

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
| Chairman cycle/outbox/external-alert recovery hardening | Implement in Chairman first | Airline has more mature stale-claim, retry, reconciliation, and lease-recovery patterns. Chairman should benefit from those ideas without forcing its schema into a shared SQL crate. | Add product-local tests and code for the concrete recovery gaps listed below. |
| Shared `world-cycle-sqlx` / `world-followup-sqlx` crates | Continue to defer | Phase 7 proved the design vocabulary but still did not prove identical authority, transaction, table, or recovery semantics. | Revisit only after Chairman hardening produces product-owned adapters matching the Phase 7 RFC sketches. |
| Event-fanout transport backend | Defer runtime extraction | Airline and Chairman still differ on Redis/pubsub versus durable outbox polling and replay expectations. | Characterize product signaling requirements; extract no transport until both products share delivery, replay, and failure semantics. |
| Idempotency runtime backend | Defer runtime extraction | Airline's runtime idempotency is Redis/incarnation-heavy; Chairman is more SQL/request-led. | Keep `idempotency-core` as key vocabulary unless both products adopt a matching runtime claim contract. |
| Tiny SQLSTATE/error classification helper | Watchlist | Both products may repeat narrow database error classification, but a broad DB helper remains too risky. | Collect duplicates while doing product work; extract only after at least two identical product call sites and product-neutral tests exist. |
| Ledger conventions | Audit only | Finance and ledger authority are still product-domain heavy. | Audit amount-sign, ledger-kind, and registry patterns. Do not add `ledger-core` unless both products expose a shared convention independent of game/business policy. |
| Auth/commercial-risk helpers | Out of scope | These are product security and business posture, not generic world infrastructure. | Keep outside Phase 8 unless a dedicated security/policy phase is explicitly approved. |

## Product-First Work

Chairman is the main Phase 8 beneficiary:

- compare Chairman cycle job repair and stale-recovery paths against Airline's
  mature cycle and post-finalization recovery behavior;
- add characterization tests before changing worker behavior;
- keep Chairman schema and enum labels product-owned;
- improve observability for stuck jobs, retry exhaustion, and repair decisions;
- preserve current outbox and external-alert delivery semantics unless a test
  proves a deliberate behavior change.

Chairman does not need to adopt Airline's `cycle_followup_retry_state` table
shape. In this audit, "post-finalization recovery" means Chairman's product-owned
outbox and external-alert delivery/recovery work.

Concrete Chairman recovery gaps to audit:

| Gap | Phase 8 goal |
| --- | --- |
| Stale cycle job reclaim | Prove stale `world_cycle_jobs` rows can be reclaimed or explicitly explain why the current repair path is enough. |
| Stuck phase state | Prove phase-state idempotency, replay safety, and recovery after a worker exits mid-phase. |
| Repair dry-run/apply visibility | Prove operators can see what repair would change before mutation and what apply actually changed. |
| Outbox drain idempotency | Prove duplicate drains do not duplicate externally visible effects. |
| External-alert lease expiry | Prove abandoned alert delivery claims become claimable again after lease expiry. |
| Retry exhaustion reporting | Prove terminal/exhausted delivery state is visible to operators and metrics. |
| Ambiguous-after-side-effect handling | Prove ambiguous provider outcomes are not silently retried or marked terminal without product policy. |
| Worker run report/operator visibility | Prove worker runs produce enough structured evidence to debug stuck cycles and delivery drains. |

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

1. Audit Chairman worker/cycle/outbox/external-alert recovery against Airline's
   mature paths.
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
- Any SQLSTATE/error classification helper has at least two identical product
  call sites and product-neutral tests before extraction.
- Deferred candidates remain documented with concrete reasons rather than vague
  "later" language.
- CI evidence is recorded for `world-infra`, Chairman, and Airline if any repo
  changes during the phase.
