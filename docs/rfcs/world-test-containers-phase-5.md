# RFC: world-test-containers Phase 5

## Status

Approved

## Summary

Create a small `world-test-containers` crate for product-neutral Docker image
wrappers used by integration tests. The crate owns image names, default tags,
and readiness waits for infrastructure services shared by persistent-world
products. It does not run product migrations, seed product data, or encode
product schemas.

## Product Evidence

- Airline adapter/example:
  `/home/dean/airline/apps/loco-app/src/testing/postgres.rs` and
  `/home/dean/airline/apps/loco-app/src/testing/redis.rs` already wrap Postgres
  and Valkey for Testcontainers-backed tests.
- Chairman adapter/example:
  Chairman has disposable Postgres tests behind
  `CHAIRMAN_TEST_DATABASE_URL`, but no reusable Testcontainers wrapper yet.
- Approved deferral:
  Chairman adoption can start with one disposable Postgres smoke. Full migration
  to container-backed database tests is not required for this crate.

## Boundaries

Product-owned:

- migrations;
- seed data;
- database URLs and pool sizing;
- local developer service requirements;
- product-specific Redis/Valkey keys and schemas.

Shared:

- image names and default tags;
- readiness waits;
- optional feature gates for runtime-heavy test dependencies.

## API Sketch

```rust
#[cfg(feature = "postgres")]
let postgres = world_test_containers::postgres::Postgres::default();

#[cfg(feature = "valkey")]
let valkey = world_test_containers::valkey::Valkey::default();
```

## Compatibility

- Semver: additive new crate.
- Runtime dependencies: disabled by default; `testcontainers` is pulled only by
  `postgres` or `valkey` features.
- SQL/wire/persisted formats: none.
- Rollback: products can return to their local image wrappers without data
  migration.

## Verification

Shared crate:

```sh
cargo test -p world-test-containers --no-default-features
cargo test -p world-test-containers --features postgres
cargo test -p world-test-containers --features valkey
cargo clippy -p world-test-containers --all-targets --all-features -- -D warnings
```

Consumer canaries:

- Airline `80b60c4c1` replaces its local Postgres and Valkey image wrappers
  while preserving existing `loco_app::test_postgres` and
  `loco_app::test_redis` exports.
- Chairman `3643c3b` adds a normal image-defaults test and an ignored disposable
  Postgres smoke without changing normal service-free local test gates.
