# RFC: Notification Provider Outcome Adapters Phase 6

Status: accepted for Phase 6 implementation.

## Decision

Extend `notification-core` around the existing `NotificationProviderOutcome`
vocabulary instead of adding a parallel accepted/retryable/permanent outcome
enum.

Phase 6 adds:

- helper constructors and predicates on `NotificationProviderOutcome`;
- failure-only `ProviderFailure` metadata for adapter code that needs a
  distinct provider failure shape;
- sanitized lowercase `ProviderFailureCode` labels for stable, non-sensitive
  provider reason codes.

## Boundaries

`ProviderFailure` is intentionally failure-only. Successful provider attempts
continue to use `NotificationProviderOutcome::Accepted`.

Shared types must not carry:

- raw provider response bodies;
- recipient email addresses;
- push subscription endpoints;
- device tokens;
- bearer/auth material;
- SMTP diagnostics;
- provider payloads or other credential-bearing detail.

Products may log redacted provider details locally and may persist product-owned
error strings according to their existing policy. Shared metadata is limited to
classification and sanitized labels.

## Product Adoption

Airline maps existing SMTP send error semantics into the shared outcome:

- local setup/build failures are not provider outcomes;
- SMTP/provider-not-accepted failures map to retryable provider failure;
- ambiguous SMTP send failures map to ambiguous-after-side-effect.

Airline maps Web Push send errors into the shared outcome:

- expired/invalid subscriptions map to permanent provider failure;
- transient Web Push errors map to retryable provider failure.

Chairman maps retryable external-alert provider failures through the shared
outcome adapter for retry scheduling. Chairman's max-attempt exhaustion remains
product-owned retry policy and is not treated as provider-specific permanent
classification.

## Non-Goals

- No shared SMTP, Web Push, FCM, APNs, or webhook clients.
- No shared notification preference, quiet-hour, category, template, or copy
  policy.
- No shared delivery SQL, notification center schema, subscription retirement,
  or partial-success policy.
- No change to Chairman's retry-until-exhaustion behavior.
- No change to Airline's notification delivery retry/failure persistence
  behavior.
