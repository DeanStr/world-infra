# Phase 0-3 Readiness Audit

Date: 2026-06-28.

Scope: phases 0, 1, 2, and 3 from
`/home/dean/chairman/docs/world-infra-unified-extraction-plan.md`.

Conclusion: phases 0-3 are implemented and locally canaried first as an
extraction spike, then against the canonical remote release-candidate source
`https://github.com/DeanStr/world-infra.git` tag `world-infra-v0.1.0-rc.2`.
They are not final-release complete until remote product canaries pass or have
approved dated deferrals.

## Phase 0: Foundation

Status: implemented for local extraction; release hardening is checked in but
not fully exercised locally.

Evidence:

- Rust workspace exists at `/home/dean/world-infra` with edition 2021, MSRV
  1.88, workspace lint policy, rust-toolchain, dual license files, and CI.
- `README.md` records shared-infra boundaries and product-owned policy.
- `GOVERNANCE.md` records the lead team, DRI, required reviewers, veto model,
  and committed-crate ownership matrix for phase 1-3 crates.
- `docs/dependency-compatibility.md` records source-product dependency versions
  and the resolved shared stack, including the OpenTelemetry 0.32 resolution.
- `docs/command-audit.md` records Chairman, Airline, and shared-workspace
  verification commands and local tool availability.
- `docs/chairman-product-hardening-tracker.md` and
  `docs/simulation-quality-tracker.md` keep adjacent work outside first-wave
  shared crates.
- `RELEASING.md`, `docs/shared-crate-readiness-checklist.md`,
  `docs/rfcs/TEMPLATE.md`, supply-chain policy, consumer-canary template, and
  compatibility-debt template are checked in.
- CI installs and runs feature-matrix, audit, license/dependency, semver, and
  secret-scanning tools.
- `scripts/pinned-product-deps.sh` generates exact-revision product dependency
  entries after the shared repo has a commit.
- Canonical remote exists at `https://github.com/DeanStr/world-infra.git`.
- Release-candidate tag exists: `world-infra-v0.1.0-rc.2`.

Local verification recorded:

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features`
- `cargo test --workspace --no-default-features`
- `cargo test --workspace --all-features --examples`
- `cargo doc --workspace --all-features --no-deps`
- `cargo audit`

Remaining before release-candidate or committed product consumption:

- Run the remote product canaries against the canonical remote immutable source
  and record ReadyCI run IDs.
- Run CI or local equivalents for `cargo-deny`, `cargo-hack`,
  `cargo-semver-checks`, and `gitleaks`. These tools are configured in CI but
  were unavailable locally on 2026-06-28.
- Repository package metadata points at
  `https://github.com/DeanStr/world-infra`.

## Phase 1: Low-Risk Proof Of Reuse

Status: implemented and locally canaried in both products where applicable.

Crates:

- `world-telemetry`
- `world-env`
- `world-test-lite`

Shared implementation evidence:

- `world-telemetry` exposes `TelemetryConfig`, exporter protocol selection,
  endpoint derivation, resource metadata, strict/best-effort failure modes, and
  `TelemetryGuard`.
- `world-env` exposes parsing primitives for required/optional values,
  strict/lenient booleans, default-with-minimum values, durations, CSV/header
  lists, and environment classification without product deployment policy.
- `world-test-lite` exposes dependency-light environment guards,
  eventual-assert retry, and fixture loading, with process-global environment
  warnings in crate docs.
- Each crate has product-neutral unit tests and an example under
  `crates/<crate>/examples`.

Product canary evidence:

- Chairman API and worker use `world-telemetry`; API and worker config helpers
  use `world-env`; tests use `world-test-lite`.
- Airline `airline-utils` uses `world-env` and `world-test-lite`.
- Airline `loco-app` tracing uses `world-telemetry` with `otlp-grpc-tonic`,
  preserving best-effort startup, default filter handling, and
  `OTEL_EXPORTER_OTLP_ENDPOINT` behavior.

Recorded local gates:

- Chairman: `cargo test -p chairman-api`, `cargo test -p chairman-worker`, and
  focused clippy for API/worker/game-db passed.
- Airline: `cargo test -p airline-utils`, `cargo test -p loco-app
  observability`, `cargo test -p loco-app telemetry_config`, and focused clippy
  passed.

## Phase 2: Identity, HTTP, Idempotency, And Clock

Status: implemented and locally canaried.

Crates:

- `world-identity-core`
- `http-primitives`
- `idempotency-core`
- `world-clock-core`

Shared implementation evidence:

- `world-identity-core` models explicit no-incarnation and required-incarnation
  world references without forcing a single world-id type.
- `idempotency-core` depends on `world-identity-core` and builds or validates
  new keys only; it does not parse, normalize, or migrate existing persisted
  product keys.
- `http-primitives` provides request-id validation, origin parsing, trusted
  proxy CIDR parsing, client-IP extraction, and public-bind predicates without
  router/framework policy.
- `world-clock-core` provides SQL-free cadence, cutoff, due-time, cycle/day, and
  deterministic seed helpers.
- Each crate has product-neutral unit tests and an example under
  `crates/<crate>/examples`.

Product canary evidence:

- Chairman tests characterize `WorldRef<Uuid, NoIncarnation>` and new-key
  construction with `idempotency-core`; API request id/client IP parsing uses
  `http-primitives`.
- Airline exposes a narrow `world_infra` adapter over
  `WorldRef<i32, IncarnationId<Uuid>>`, proving durable-world identity cannot
  omit `world_instance_id`.
- Airline uses `idempotency-core` only for new world-instance-scoped key
  construction.
- Airline request IP extraction delegates trusted proxy CIDR parsing and
  forwarded-header precedence to `http-primitives`.
- Airline uses `world-clock-core::Cadence` inside its existing next-cycle start
  helper while retaining scheduler authority and cycle policy locally.

Recorded local gates:

- Chairman: `cargo test -p chairman-api` passed.
- Airline: `cargo test -p loco-app world_infra`, `cargo test -p loco-app
  utils::net`, and `cargo test -p loco-app next_cycle_start_time` passed.

## Phase 3: Runtime Mechanics And Tenant Scope

Status: implemented and locally canaried.

Crates:

- `rate-limit-core`
- `tenant-scope-sqlx`
- `delivery-core`

Shared implementation evidence:

- `rate-limit-core` provides `LimitSpec`, in-process and optional Redis
  fixed-window backend mechanics, namespace/key validation, retry-after
  decisions, backend health, failure policy, and metrics hooks while leaving
  route/tier/account policy local.
- `tenant-scope-sqlx` provides validated transaction-bound PostgreSQL
  `SET LOCAL` statement construction and execution, with setting-name and
  setting-value safety tests. It does not provide pool wrappers, transaction
  factories, product setting names, or RLS assumptions.
- `delivery-core` provides status vocabulary, lease tokens, claimed delivery
  envelopes, optional world-owned envelopes, attempt outcomes including
  ambiguous-after-side-effect, run reports, retry/backoff helpers, and
  claim/finalize traits.
- Each crate has product-neutral unit tests and an example under
  `crates/<crate>/examples`.

Product canary evidence:

- Chairman API rate-limit backends use `rate-limit-core`, while classes, keys,
  fallback policy, and response shape remain local.
- Chairman `chairman-game-db` world transaction scope uses
  `tenant-scope-sqlx`.
- Chairman worker external-alert retry/backoff vocabulary uses `delivery-core`,
  while provider adapters, SQL tables, categories, and copy remain local.
- Airline rate-limit middleware and runtime use `rate-limit-core` for
  in-process and Redis fixed-window backend mechanics, while route labels, tier
  labels, account lookup, fail-open/fail-closed policy, metrics, health response
  shape, and `0`-means-force-closed adapter behavior remain local.
- Airline `loco-app` and `sim-engine` use `tenant-scope-sqlx` to build reviewed
  tenant-scope statements while keeping world and airline setting names local.
- Airline notification delivery uses `delivery-core::BackoffPolicy` for retry
  timing, `ClaimedDelivery` and `LeaseToken` around immediate and daily-digest
  claim paths, and `DeliveryFinalizer` plus `DeliveryAttemptOutcome` underneath
  existing sent/retry/failed mark helpers.
- Airline ambiguous-after-side-effect outcomes are represented and are not
  auto-finalized.

Recorded local gates:

- Chairman: `cargo test -p chairman-api`, `cargo test -p chairman-worker`,
  `cargo test -p chairman-game-db --lib`, and focused clippy passed.
- Airline: `cargo test -p loco-app rate_limit`,
  `cargo test -p loco-app --test rate_limit_inproc`,
  `cargo test -p loco-app-it --test rate_limit_redis --features tc`,
  `cargo test -p loco-app db::rls`, `cargo test -p sim-engine db::rls`,
  `cargo test -p loco-app notification_delivery_backoff`,
  `cargo test -p loco-app notification_delivery`, and focused clippy passed.

## Final Release Gap

The code and local canary evidence are sufficient for an extraction spike and a
remote exact-revision release-candidate canary, but not for final release.

Observed blocker:

- Airline ReadyCI runs previously reached remote execution but failed during
  Cargo metadata resolution because `/world-infra` was not present in the remote
  workspace.
- Product checkouts now need to rerun ReadyCI against
  `https://github.com/DeanStr/world-infra.git` tag `world-infra-v0.1.0-rc.2`.

Required next release steps:

1. Run the shared CI gates and the product canary commands recorded in
   `docs/consumer-canaries/`.
2. Record exact revisions, remote run IDs, failures, and disposition.
3. Tag final releases only after shared CI is green and both product canaries
   pass, or after approved dated deferrals are recorded.
