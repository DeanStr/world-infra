# Consumer Canary: Chairman Phase 1-3 Local Git Revision Adoption

Crates:

- `world-telemetry`
- `world-env`
- `world-test-lite`
- `world-identity-core`
- `http-primitives`
- `idempotency-core`
- `rate-limit-core`
- `tenant-scope-sqlx`
- `delivery-core`

Candidate revision or tag:

- local extraction spike: path dependency from `/home/dean/chairman` to
  `/home/dean/world-infra`;
- release-candidate remote git proof:
  `https://github.com/DeanStr/world-infra.git` tag
  `world-infra-v0.1.0-rc.3`.

Consumer: Chairman.

Owner: Chairman world-infra DRI.

Date: 2026-06-27; exact-revision rerun on 2026-06-28.

Scope:

- API and worker telemetry adapters now use `world-telemetry`.
- API and worker config helpers use `world-env`.
- API request-id and client-IP parsing use `http-primitives`.
- API tests characterize `WorldRef<Uuid, NoIncarnation>` and new-key
  construction with `idempotency-core`.
- API rate-limit backends use `rate-limit-core`; Chairman still owns classes,
  keys, fallback, and response shape.
- `chairman-game-db` world transaction scope uses `tenant-scope-sqlx`.
- Worker alert retry/backoff vocabulary uses `delivery-core`; Chairman still
  owns SQL tables, provider adapters, notification categories, and user copy.

Commands run:

```sh
cargo test -p chairman-api
cargo test -p chairman-worker
cargo test -p chairman-game-db --lib
cargo clippy -p chairman-api -p chairman-worker -p chairman-game-db --all-targets -- -D warnings
cargo tree -p chairman-api -i world-telemetry
cargo tree -p chairman-worker -i delivery-core
cargo tree -p chairman-game-db -i tenant-scope-sqlx
```

Results:

- `chairman-api`: 15 tests passed.
- `chairman-worker`: 7 tests passed; `seed-dev-world` compiled.
- `chairman-game-db`: 65 tests passed, 1 ignored Postgres-backed test.
- Focused clippy gate passed with `-D warnings`.
- Exact-revision rerun compiled shared crates from the remote
  `world-infra-v0.1.0-rc.3` candidate.
- Dependency tree confirms `chairman-api`, `chairman-worker`, and
  `chairman-game-db` consume the pinned git source for `world-telemetry`,
  `delivery-core`, and `tenant-scope-sqlx`.

Failures and disposition:

- Initial wiring failures for binary-only `chairman-api --lib`, missing test
  dependency, stale imports, and test-module placement were fixed.

Release decision:

- Suitable as local extraction-spike and remote exact-revision evidence.
- Final release still requires shared CI and both product canaries or approved
  dated deferrals.
