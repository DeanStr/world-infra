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
- product-neutral world clock primitives;
- rate-limit backend mechanics;
- narrow transaction-bound SQLx scope helpers;
- durable delivery worker vocabulary;
- product-neutral event envelope metadata;
- notification delivery channel, target, version, and provider outcome
  vocabulary;
- optional Testcontainers image wrappers for product integration tests.

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
- `docs/phase-5-reassessment-audit.md`.
