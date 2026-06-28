# Compatibility Debt Deferral: Airline Release-Canary Remote Verification

Owner: Airline platform owner.

Due date: 2026-08-15.

Status: obsolete as of 2026-06-28. Network-enabled ReadyCI canaries supersede
this proposed deferral.

Affected crate/version:

- Phase 1-3 canaried `world-infra` crates at `0.1.0`

Reason:

Chairman has completed local-path and remote exact-revision canary adoption for
phase 1-3 surfaces. Airline now has local-path and remote exact-revision canary
adoption for `world-env`,
`world-test-lite`, `world-telemetry`, `http-primitives`, `rate-limit-core`,
`tenant-scope-sqlx`, `world-clock-core`, and `delivery-core`, recorded in
`docs/consumer-canaries/airline-phase-1-3-local-path.md`.
The rc8 validation also covered `world-identity-core` and `idempotency-core`
through a canary-only Airline adapter that was removed after validation because
it had no production callers.

ReadyCI remote wrappers originally reached the affected Rust rules but failed at
Cargo metadata because the remote workspace could not resolve local
`/world-infra` path dependencies. That class of blocker is addressed by
publishing the shared repository at
`https://github.com/DeanStr/world-infra.git` and repinning products to the
`world-infra-v0.1.0-rc.8` canonical release-candidate revision.

Subsequent exact-revision ReadyCI attempts submitted with no usable network path
failed before compilation while Cargo fetched `world-infra` from `github.com`.
Those DNS failures are expected under `network_mode=none` and do not indicate a
Rust or shared-crate compatibility failure. Network-enabled reruns supersede
this deferral:

- `READYCI_RUN_FLAGS='--network-mode default' make remote-rust-check-pkg
  P=airline-utils` passed, including 19 tests.
- `run_66be0549bdce24d2` passed the quiet `loco-app` `test-support` test lane
  with `network_mode=default` and `runner_size=large`.
- `run_9625b0ff9f1a9794` passed `sim-engine` fmt, quiet clippy, and the focused
  `db::rls` test with `network_mode=default` and `runner_size=large`.

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
- Cargo git dependency checks intentionally require `network_mode=default` or an
  equivalent warm/cold-cache path. They are expected to fail under
  `network_mode=none`.

What makes this obsolete:

- Network-enabled Airline remote package canaries pass against the canonical
  remote immutable source.

Scheduled review date: 2026-07-27.

Approved by: not required; this deferral was superseded before approval.

Historical next verification, now superseded by the network-enabled runs above:

```sh
make remote-rust-check-pkg P=airline-utils
make remote-rust-check-pkg P=loco-app
make remote-rust-check-pkg P=loco-app-it
make remote-check-touched
```

Use narrower package names if Airline chooses crate-by-crate adoption.
