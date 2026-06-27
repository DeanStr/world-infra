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
- `cargo-deny`: not installed locally when rechecked on 2026-06-28; configured
  in shared CI.
- `cargo-hack`: not installed locally when rechecked on 2026-06-28; configured
  in shared CI for feature matrix coverage.
- `cargo-semver-checks`: not installed locally when rechecked on 2026-06-28;
  configured in shared CI as an initial non-blocking baseline check.
- `gitleaks`: not installed locally when rechecked on 2026-06-28; configured in
  shared CI for secret scanning.

## Release-Candidate Pinning Check

Observed on 2026-06-28:

- `scripts/pinned-product-deps.sh` generated exact-revision dependency entries
  for local canaries after the initial `world-infra` commit
  `a3882e8cca0bf8a4c38f1c2868e52a6031f70dd5`.
- Airline `make remote-check-touched` and `make remote-rust-check-pkg
  P=airline-utils` reached ReadyCI but failed at Cargo metadata because local
  `/world-infra` path dependencies are not available in the remote workspace.
  This confirms remote canaries require exact-revision git dependencies from a
  canonical remote URL, not sibling local paths or local `file://` URLs.
