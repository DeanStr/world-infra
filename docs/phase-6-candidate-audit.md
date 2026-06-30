# Phase 6 Candidate Audit

Date: 2026-06-29

Scope: post-Phase 5 reassessment across `/home/dean/world-infra`,
`/home/dean/chairman`, and `/home/dean/airline`. This audit records the chosen
implementation slice plus the resulting release/product canary evidence.

Conclusion: Phase 6 is implemented as a narrow notification provider-outcome
adapter slice in `notification-core`. Broad SQL extraction remains deferred.
The heavier candidates still need trait/RFC work before code because Airline
and Chairman have intentionally different persistence authority and world
identity boundaries.

## Recommended Phase 6 Slice

Extend the existing provider outcome vocabulary in `notification-core`.

The shared crate already owns product-neutral notification channels, delivery
versions, positive provider attempts, delivery contexts, and
`NotificationProviderOutcome`. Both products also use `delivery-core` adapters
for notification/external-alert finalization. What remains duplicated is the
last small adapter step between provider-specific failures and the existing
shared outcome type.

RFC: `docs/rfcs/notification-provider-outcome-adapters-phase-6.md`.

Boundaries for this slice:

- do not add a second enum that repeats
  `NotificationProviderOutcome::{Accepted, RetryableFailure, PermanentFailure,
  AmbiguousAfterSideEffect}`;
- prefer helper constructors, docs, and tests on the existing
  `NotificationProviderOutcome` vocabulary;
- if a new type is needed, make it failure-only and distinct, such as provider
  failure class/rationale metadata, not a parallel accepted/retryable/permanent
  outcome enum;
- include optional retry-after metadata where a provider supplies it;
- include helpers that preserve the existing conversion to
  `delivery_core::DeliveryAttemptOutcome`;
- do not carry raw provider response bodies, email addresses, subscription
  endpoints, device tokens, bearer/auth material, SMTP diagnostics, or other
  sensitive provider details in shared types;
- keep provider clients, SMTP/WebPush/FCM/APNs request construction, payload
  shape, subscription retirement, notification categories, preferences, quiet
  hours, and delivery SQL local;
- require product adapters to make provider-specific decisions explicitly
  rather than auto-classifying arbitrary errors in world-infra.

Product evidence:

- Airline already distinguishes local email setup/build failures, retryable
  SMTP/provider failures, permanent SMTP/provider rejections, and ambiguous send
  outcomes in `apps/loco-app/src/services/email.rs`, and Web Push errors
  distinguish permanent expired subscriptions from transient send failures in
  `apps/loco-app/src/services/push.rs`.
- Chairman currently treats external alert provider failures through the
  delivery retry/exhaustion policy in `apps/chairman-worker/src/delivery_utils.rs`
  and provider adapters in `apps/chairman-worker/src/smtp_delivery.rs` and
  `apps/chairman-worker/src/native_push.rs`.
- Neither product needs shared provider clients for this step. They only need a
  common outcome vocabulary at the adapter boundary.

Acceptance gates:

- Airline SMTP tests cover local/pre-provider failures, retryable provider
  failures, permanent provider rejections, and ambiguous outcomes without moving
  SMTP client behavior into world-infra.
- Airline Web Push tests cover permanently invalid subscriptions versus
  transient send failures while keeping subscription retirement product-owned.
- Chairman external-alert tests prove current retry/exhaustion behavior is
  unchanged: provider failures continue to retry until the product max-attempt
  policy makes them terminal, unless Chairman explicitly opts into a
  provider-specific permanent classification.
- Chairman webhook, SMTP, FCM, APNs, and browser-push adapters map through
  shared helpers only where semantics are explicit.
- Multi-recipient or multi-device partial-success behavior remains
  product-owned in both products.
- Shared classification metadata is sanitized and never contains raw provider
  bodies, endpoints, recipient addresses, tokens, or credential-bearing detail.

Suggested verification:

- `cargo test -p notification-core`
- `cargo clippy -p notification-core --all-targets -- -D warnings`
- focused Airline tests around email/push notification delivery classification
- focused Chairman tests around external alert delivery finalization and native
  push/webhook failures

## Candidate Decisions

| Candidate | Decision | Reason | Next action |
| --- | --- | --- | --- |
| `notification-core` provider outcome adapters | Implement first | Existing shared notification and delivery vocabulary is stable in both products; the missing piece is small and product-neutral. | Reuse `NotificationProviderOutcome`; add helper constructors/docs/tests or distinct failure-only metadata, then adopt through product adapters. |
| `world-cycle-sqlx` | Defer implementation | Airline's cycle SQL is incarnation-aware and recovery-heavy; Chairman's current model is job/outbox-oriented and maps shared statuses onto product enum labels. Shared SQL would freeze table semantics too early. | Write a Phase 6 RFC/trait sketch only if both products want to converge on adapter boundaries. |
| `world-followup-sqlx` | Defer implementation | Airline has a durable `cycle_followup_retry_state` table with leases and retry scheduling; Chairman currently uses outbox and external-alert delivery rows rather than a generic follow-up table. | Revisit after Chairman either adopts a generic durable follow-up table or maps existing rows into a shared trait contract. |
| `event-fanout` transport backends | Defer | Phase 5 created vocabulary only. Airline has Redis/pubsub and durable cycle-completion markers; Chairman currently relies on durable outbox polling and does not need a shared Redis transport yet. | Keep product transports local; only add shared helpers after both products use the same runtime signaling shape. |
| Redis/in-memory idempotency backends | Deferred in Phase 6; superseded post-plan | Airline's Redis idempotency is runtime critical and incarnation-scoped; Chairman's idempotency is primarily SQL ledger/request handling. A shared backend would either be Airline-shaped or too abstract. | Phase 6 kept `idempotency-core` to key construction/validation. The later `docs/rfcs/idempotency-runtime-core.md` RFC approves a narrow volatile runtime claim-store crate and keeps durable SQL/request idempotency local. |
| Broad `world-db-sqlx` helpers | Defer | `FOR UPDATE SKIP LOCKED`, `ON CONFLICT`, migration, and pool code are common patterns but not common semantics. | Consider only a tiny SQLSTATE/error-classification helper if repeated identical parsing emerges. |
| `ledger-core` | Later | Finance ledgers are product-domain heavy: owner wealth, club finance, airline finance, and ledger kinds differ. | Reassess for amount-sign conventions or registry patterns after both products expose matching ledger abstractions. |
| Auth/commercial-risk helpers | Separate lane | These touch product policy and security posture more than shared world infrastructure. | Keep outside world-infra unless both products approve a dedicated phase. |

## Guardrails Learned From Phase 5

- Do not extract SQL table names, enum labels, or claim queries unless both
  products already share the same authority boundary.
- Preserve Airline's `world.instance_id`/incarnation safety for durable cycle,
  event, delivery, and idempotency flows.
- Do not force Chairman to adopt Airline's mature recovery tables just to make a
  shared crate look useful.
- Do not let shared crates own provider clients or user-facing notification
  policy.
- Do not let shared notification types carry raw provider bodies, recipient
  addresses, endpoints, tokens, credential material, or unsanitized diagnostics.
- Do not reinterpret Chairman's current attempt-count exhaustion policy as a
  provider-permanent rejection unless Chairman explicitly adopts that behavior.
- Prefer vocabulary and adapter contracts first; extract persistence only after
  product code proves the same contract twice.

## Phase 6 Implementation Order

1. Add a small RFC for notification provider outcome adapters.
2. Implement helper constructors/docs/tests on `NotificationProviderOutcome`, or
   a distinct failure-only metadata type if the RFC proves it adds value.
3. Adopt the helper in Airline email/Web Push notification delivery paths.
4. Adopt the helper in Chairman external alert SMTP/WebPush/FCM/APNs/webhook
   delivery paths where it improves clarity without changing product retry
   policy.
5. Record focused product canary evidence.
6. Separately write a `world-cycle-sqlx`/`world-followup-sqlx` trait-boundary
   RFC before any SQL crate implementation.

## Phase 6 Implementation Notes

Implemented shared surface:

- `NotificationProviderOutcome` helper constructors and predicates;
- `ProviderFailure` as failure-only sanitized adapter metadata;
- `ProviderFailureCode` as a constrained lowercase stable label type that
  rejects raw-provider-detail characters such as `/`, `@`, `:`, `.`, and
  whitespace.

Implemented product adoption:

- Airline maps `EmailSendError` local/pre-provider failures to no provider
  outcome, retryable SMTP/provider failures to retryable provider outcomes,
  permanent SMTP/provider rejections to permanent provider outcomes, and
  ambiguous SMTP outcomes to ambiguous provider outcomes.
- Airline maps `WebPushSendError` transient/permanent outcomes into shared
  provider outcomes and uses that outcome in notification push delivery.
- Chairman maps retryable external-alert provider failures through shared
  outcome helpers while keeping max-attempt exhaustion product-owned.

Final verification evidence:

- Release:
  - tag: `world-infra-v0.1.0-rc.15`
  - shared source: `c7e55952da397856f945f232d8816c347c6dd9fb`
  - Airline canary: `f3b52ecbe2e7d44283dd55674c212552acff52b7`
  - Chairman canary: `6a60862b197416cfe52701d73332a272623693ce`
- world-infra:
  - `cargo test -p notification-core`
  - `cargo clippy -p notification-core --all-targets -- -D warnings`
- Airline:
  - `cargo test -p loco-app smtp_send_errors_map_to_shared_provider_outcomes`
  - `cargo test -p loco-app web_push_errors_map_to_shared_provider_outcomes`
  - `cargo clippy -p loco-app --all-targets -- -D warnings`
- Chairman:
  - `cargo test -p chairman-worker alert_retry_backoff_is_bounded_and_terminal`
  - `cargo test -p chairman-worker alert_provider_outcome_adapter_does_not_change_exhaustion_policy`
  - `cargo clippy -p chairman-worker --all-targets -- -D warnings`

Cargo emitted existing local path-override warnings in Airline and Chairman
because those workspaces override pinned world-infra git dependencies to
`/home/dean/world-infra` for iteration. The focused checks completed
successfully.

GitHub CI evidence:

- world-infra `ci` passed for `c7e55952da397856f945f232d8816c347c6dd9fb`
  in run `28372133625`.
- Airline `ci` passed for `f3b52ecbe2e7d44283dd55674c212552acff52b7`
  in run `28374961909`.
- Chairman `CI` passed for `6a60862b197416cfe52701d73332a272623693ce`
  in run `28374960875`.
