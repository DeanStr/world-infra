# Command Audit

This audit verifies commands named in the extraction plan against the local
checkouts.

## Repositories

- Chairman local path: `/home/dean/chairman`
- Airline local path: `/home/dean/airline`
- Shared workspace path: `/home/dean/world-infra`

The Airline upstream repository identity may differ from the local directory
name; local verification should use the commands documented in
`/home/dean/airline/AGENTS.md` until the remote origin is confirmed.

## Chairman Commands

Observed in `/home/dean/chairman/package.json` or project guidance:

- `pnpm verify:fast`
- `pnpm verify`
- `pnpm verify:ops`
- `pnpm api:openapi:check`
- `pnpm api:types:check`
- `pnpm web:typecheck`
- `pnpm web:lint`
- `pnpm web:test`
- `pnpm web:build`
- Rust package checks through Cargo package names `chairman-api`,
  `chairman-worker`, `chairman-game`, and `chairman-game-db`.

## Airline Commands

Observed in `/home/dean/airline/AGENTS.md`:

- `make remote-check-touched`
- `make remote-rust-check-pkg P=<cargo-package>`
- `make remote-check-ci`
- `make remote-check-ci-db`
- `make remote-check-ci-db-parallel`
- `make remote-web-typecheck`
- `make remote-web-lint`
- `make remote-web-test`
- `make remote-web-build`
- `make remote-web-contracts`
- `make remote-web-check`
- `make remote-check-all`
- `make remote-check-all-db`
- `make remote-release-check-fast`

## Shared Workspace Commands

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features`
- `cargo test --workspace --no-default-features`
- `cargo doc --workspace --all-features --no-deps`
- `scripts/pinned-product-deps.sh`: generates exact-revision product dependency
  entries after the shared repository has a release-candidate commit.

## Local Tool Availability

Observed on 2026-06-28:

- `cargo-audit`: available locally and run successfully against
  `/home/dean/world-infra/Cargo.lock`.
- `cargo-deny`: installed later on 2026-06-28; `cargo deny check licenses`
  passed locally after the license allowlist was updated. Full deny checks are
  also configured in shared CI.
- `cargo-hack`: not installed locally when rechecked on 2026-06-28; configured
  in shared CI for feature matrix coverage.
- `cargo-semver-checks`: not installed locally when rechecked on 2026-06-28;
  configured in shared CI as an initial non-blocking baseline check.
- `gitleaks`: not installed locally when rechecked on 2026-06-28; configured in
  shared CI for secret scanning.

## Release-Candidate Pinning Check

Observed on 2026-06-28:

- `scripts/pinned-product-deps.sh` generated exact-revision dependency entries
  for the canonical `https://github.com/DeanStr/world-infra.git` remote after
  the release-candidate tag `world-infra-v0.1.0-rc.6` was created.
- Airline `make remote-check-touched` and `make remote-rust-check-pkg
  P=airline-utils` reached ReadyCI but failed at Cargo metadata because local
  `/world-infra` path dependencies are not available in the remote workspace.
  This confirmed remote canaries require exact-revision git dependencies from a
  canonical remote URL, not sibling local paths or local `file://` URLs.
- After product manifests were repinned to the canonical GitHub exact revision,
  ReadyCI runs submitted with `network_mode=none` failed before compilation
  because Cargo git dependencies intentionally cannot resolve `github.com`
  without a network path. Observed no-network run ids include
  `run_bdeb84769cf649f4`, `run_5959f87fcf0482e9`, and
  `run_5f2449049e679631`; the last was retried with
  `CARGO_NET_GIT_FETCH_WITH_CLI=true`, which cannot restore DNS in a no-network
  guest.
- `READYCI_RUN_FLAGS='--network-mode default' make remote-rust-check-pkg
  P=airline-utils` passed against the canonical GitHub exact revision, including
  19 tests.
- Network-enabled ReadyCI run `run_66be0549bdce24d2` passed the quiet
  `loco-app` `test-support` test lane with `network_mode=default` and
  `runner_size=large`.
- Network-enabled ReadyCI run `run_9625b0ff9f1a9794` passed `sim-engine` fmt,
  quiet clippy, and the focused `db::rls` test with `network_mode=default` and
  `runner_size=large`.
- Verbose full-package ReadyCI runs for `loco-app` and `sim-engine` reached Rust
  work but were cancelled by ReadyCI log-delivery timeouts. Quiet or focused
  network-enabled reruns provided the dependency-fetching canary evidence.
- Shared repository release evidence for `world-infra-v0.1.0-rc.6` is the local
  shared gate set plus the product exact-revision canaries recorded here.
