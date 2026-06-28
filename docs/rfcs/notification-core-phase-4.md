# Notification Core Phase 4 RFC

Date: 2026-06-28.

## Decision

Create `notification-core` as a narrow shared crate for product-neutral
notification delivery vocabulary.

The crate owns:

- external channel labels: email, browser push, FCM, APNS, webhook;
- positive notification delivery versions;
- generic target string validation;
- provider attempt outcomes that convert into `delivery-core` outcomes;
- claimed-delivery context metadata.

The crate does not own:

- notification categories, preferences, quiet hours, or inbox state;
- templates, user copy, deep links, managed actions, or product UX semantics;
- provider clients, SQL schemas, queue names, or durable finalization policy.

## Why Now

Airline has a mature notification center with delivery versions and multiple
external channels. Chairman has a newer external alert delivery path with the
same external channel vocabulary and retry/finalization shape. `delivery-core`
already proved the shared worker boundary, so the next stable extraction is the
metadata that sits between product claims and provider attempts.

## Product Canaries

Airline should use the shared delivery version and channel types around
notification delivery work items and continue to keep category, severity,
managed action, read state, and SQL shape local.

Chairman should map external alert delivery claims into shared notification
delivery context in tests or adapters, while keeping alert category, urgency,
payload, and recipient policy local.

## Rollback

Products can inline the small enums and newtypes without data migration because
`notification-core` does not define storage layout, serialized wire format, or
provider side effects.
