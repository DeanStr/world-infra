# Compatibility Debt Deferral: Airline Release-Canary Remote Verification

Owner: Airline platform owner.

Due date: 2026-08-15.

Affected crate/version:

- Phase 1-3 canaried `world-infra` crates at `0.1.0`

Reason:

Chairman has completed local-path and remote exact-revision canary adoption for
phase 1-3 surfaces. Airline now has local-path and remote exact-revision canary
adoption for `world-env`,
`world-test-lite`, `world-telemetry`, `world-identity-core`,
`idempotency-core`, `http-primitives`, `rate-limit-core`, `tenant-scope-sqlx`,
`world-clock-core`, and `delivery-core`, recorded in
`docs/consumer-canaries/airline-phase-1-3-local-path.md`.

The remaining Airline phase 1-3 work is remote verification. ReadyCI remote
wrappers originally reached the affected Rust rules but failed at Cargo metadata
because the remote workspace could not resolve local `/world-infra` path
dependencies. That class of blocker is addressed by publishing the shared
repository at `https://github.com/DeanStr/world-infra.git` and repinning products
to the `world-infra-v0.1.0-rc.5` canonical exact revision. Subsequent ReadyCI
attempts reached the remote runner but failed before compilation because the
runner could not resolve `github.com` while Cargo fetched `world-infra`.

Airline telemetry is no longer deferred: the local canary upgraded `loco-app` to
the shared OpenTelemetry `0.32` stack through `world-telemetry` while preserving
best-effort startup and gRPC/tonic OTLP endpoint behavior. Delivery-core runtime
adoption is no longer deferred: notification delivery now uses shared backoff,
claim envelopes, finalization traits, and outcome vocabulary while preserving
Airline's provider semantics, SQL claim-token flow, row statuses, and operator
workflows.

Compatibility being preserved:

- Airline's required `(world_id, world_instance_id)` durable-world invariant.
- Airline's best-effort tracing startup behavior and gRPC/tonic OTLP posture.
- Airline route labels, tier labels, account/pre-auth lookup, and response
  semantics for rate limiting. The canary preserves these locally while sharing
  backend mechanics only.
- Airline cycle scheduler authority, finalization boundary, recovery policy,
  and world-instance guards. The canary preserves these locally while sharing
  cadence validation only.
- Airline notification and email delivery table schemas, provider semantics, and
  operator workflows. The current canary preserves these locally while sharing
  retry-backoff mechanics, claim envelopes, finalization traits, and outcome
  vocabulary.
- Airline remote verification is intentionally not treated as passing until
  ReadyCI succeeds against the canonical remote immutable source.

What makes this obsolete:

- Airline remote package/touched gates pass or are replaced by an approved,
  dated remote-verification deferral for the release candidate.

Scheduled review date: 2026-07-27.

Approved by: pending Airline owner and Chairman world-infra DRI review.

Required next verification:

```sh
make remote-rust-check-pkg P=airline-utils
make remote-rust-check-pkg P=loco-app
make remote-rust-check-pkg P=loco-app-it
make remote-check-touched
```

Use narrower package names if Airline chooses crate-by-crate adoption.
