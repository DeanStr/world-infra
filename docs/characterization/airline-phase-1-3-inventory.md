# Airline Phase 1-3 Inventory

Date: 2026-06-28.

Source checkout: `/home/dean/airline`.

## Dependency Snapshot

Observed in `Cargo.toml` files:

- `sqlx`: `0.8`
- `redis`: `1`
- `axum`: `0.8`
- `tower-http`: `0.6`
- `tracing`: `0.1`
- `opentelemetry`: upgraded from `0.31` to `0.32` in `apps/loco-app` through
  `world-telemetry`
- `opentelemetry-otlp`: upgraded from `0.31` to `0.32` in `apps/loco-app`
  through `world-telemetry`, preserving `grpc-tonic`
- `reqwest`: `0.13`
- `serde`: `1`
- `time`: `0.3`

## Phase 1 Notes

Telemetry source material:

- `apps/loco-app/src/observability/tracing.rs`
- Current behavior is best-effort startup, default filter `info`, and
  `OTEL_EXPORTER_OTLP_ENDPOINT` using tonic/gRPC exporter setup.

Environment source material:

- `docs/env-config.md`
- `apps/airline-utils/src`
- `apps/loco-app/src`

Product deployment guards remain local.

Local-path canary status:

- `apps/airline-utils` consumes `world-env` behind its existing product-local
  helper functions for lenient booleans, optional scalar parsing, and compact
  duration parsing.
- `apps/airline-utils` consumes `world-test-lite` in tests for environment
  mutation cleanup.
- `apps/loco-app/src/observability/tracing.rs` consumes `world-telemetry` with
  the `otlp-grpc-tonic` feature.
- Airline telemetry preserves best-effort startup, default `info` filtering, and
  `OTEL_EXPORTER_OTLP_ENDPOINT` gRPC/tonic behavior while using the shared
  OpenTelemetry `0.32` stack.

## Phase 2 Notes

Airline durable world operations must preserve required `world_instance_id`
semantics. Shared `world-identity-core` supports this as
`WorldRef<i32, IncarnationId<Uuid>>`, but Airline adapter code must not expose a
no-incarnation fallback for durable event, delivery, idempotency, or cycle
operations.

Local-path canary status:

- `apps/loco-app/src/world_infra.rs` adapts Airline identity as
  `WorldRef<i32, IncarnationId<Uuid>>`.
- `apps/loco-app/src/world_infra.rs` uses `idempotency-core` only for new
  world-instance-scoped keys.
- `apps/loco-app/src/utils/net.rs` uses `http-primitives` for trusted proxy CIDR
  parsing and forwarded-header client IP extraction, while retaining Airline's
  `TRUST_PROXY_HEADERS`, `TRUSTED_PROXY_CIDRS`, and
  `ALLOW_UNSCOPED_PROXY_HEADERS` gates.
- Existing Airline Redis, cycle, event, and delivery keys remain unchanged.

## Phase 3 Notes

Rate-limit and delivery source material lives in `apps/loco-app`. Airline route
labels, tier labels, account/pre-auth lookup, response shapes, email retry
lineage, notification tables, and operator workflows remain product-owned.

Cycle and scheduler recovery logic is intentionally not part of phase 1-3 shared
crate adoption.

Local-path canary status:

- `apps/loco-app` consumes `rate-limit-core` for in-process and Redis
  fixed-window backend mechanics.
- Airline keeps route labels, tier labels, account/pre-auth lookup, account
  hashing, fail-open/fail-closed policy, metrics, health response shape, and
  force-closed zero-limit adapter behavior local.
- `apps/loco-app/src/db/rls.rs` and `apps/sim-engine/src/db/rls.rs` consume
  `tenant-scope-sqlx` to build reviewed `SET LOCAL app.* = ...` statements while
  preserving product-local scope names and call sites.
- `apps/loco-app/src/repo/world_clock.rs` consumes `world-clock-core::Cadence`
  inside the existing next-cycle helper. Airline keeps scheduler authority,
  pause controls, lease handling, finalization boundaries, recovery policy, and
  world-instance guards local.
- `apps/loco-app/src/services/notification_delivery/backoff.rs` consumes
  `delivery-core::BackoffPolicy` for notification delivery retry delays while
  preserving Airline's existing delay sequence and product-owned row status
  transitions.
- `apps/loco-app/src/services/notification_delivery/worker.rs` wraps immediate
  and daily-digest notification delivery SQL claims in
  `delivery-core::ClaimedDelivery`, using the existing UUID processing token as
  the shared `LeaseToken`.
- `apps/loco-app/src/services/notification_delivery/worker.rs` uses
  `delivery-core::DeliveryFinalizer` and `DeliveryAttemptOutcome` underneath the
  existing sent/retry/failed mark helpers; digest email delivery reuses those
  helpers.
- Airline notification delivery keeps product-owned SQL, preferences, quiet
  hours, tier gates, provider sends, retry-vs-failed policy, and operator
  workflows local.
