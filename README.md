# world-infra

`world-infra` is a Rust workspace for product-neutral infrastructure shared by
multiplayer persistent-world products. It deliberately does not contain football
or airline simulation logic, product auth policy, billing policy, UI copy,
domain event payloads, or product-specific persistence schemas.

The first extraction wave covers:

- telemetry setup;
- environment parsing primitives;
- lightweight test helpers;
- incarnation-aware world identity;
- HTTP request parsing and edge-safety helpers;
- idempotency key construction for new keys;
- volatile idempotency runtime claim stores;
- product-neutral world clock primitives;
- rate-limit backend mechanics;
- narrow transaction-bound SQLx scope helpers;
- durable delivery worker vocabulary;
- cycle runtime status, lease, phase, and follow-up retry primitives;
- product-neutral event envelope metadata;
- event fanout source/outcome classification;
- notification delivery channel, target, version, and provider outcome
  vocabulary;
- optional Testcontainers image wrappers for product integration tests.

The post-plan product-hardening additions cover:

- JSON/API contract-test helpers;
- static web runtime-config and security-header checks;
- scoped SQL review linting;
- operator dead-letter and repair report vocabulary;
- upload key, content-type, size, and completion-state primitives;
- auth claim/session/origin primitives;
- password hashing and password-policy helpers;
- opaque refresh-token minting and single-use rotation stores;
- HTTP refresh-cookie and trusted-origin helpers;
- WebSocket authentication handshake and control-frame primitives;
- provider-neutral billing risk and chargeback evidence helpers;
- notification lifecycle, recurrence, delivery-version, and broadcast claim
  helpers.

Product adapters remain in product repositories. Shared crates expose typed
library errors and product-neutral examples; products map those errors to their
own API responses, logs, migrations, and operator workflows.

## Workspace Policy

- Rust edition: 2021.
- MSRV: Rust 1.88, which supports the current Redis/tonic integration stack while
  keeping shared crates on Rust edition 2021.
- Runtime-heavy integrations are optional features.
- `world-test-lite` is for dev/test use only.
- `world-test-containers` is for dev/test use only and has no default features.
- Reassessment candidates must not get crate shells without a design note and
  both-product approval.

## Verification

Initial local checks:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test --workspace --no-default-features
cargo doc --workspace --all-features --no-deps
```

## Product Consumption

Use local path dependencies only for extraction spikes. For release-candidate or
committed product work, generate immutable git dependency entries with:

```sh
scripts/pinned-product-deps.sh
```

See `docs/release-candidate-consumption.md` for the product canary flow.
The current implementation/readiness state is recorded in:

- `docs/phase-0-3-readiness-audit.md`;
- `docs/phase-4-readiness-audit.md`;
- `docs/phase-5-reassessment-audit.md`;
- `docs/phase-6-candidate-audit.md`;
- `docs/phase-7-candidate-audit.md`;
- `docs/phase-8-candidate-audit.md`.

The numbered extraction plan is closed through Phase 8. Future shared work
should start as concrete backlog items, candidate audits, or narrow RFCs rather
than as a Phase 9 by default.
