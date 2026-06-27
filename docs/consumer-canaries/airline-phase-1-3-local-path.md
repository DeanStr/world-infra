# Consumer Canary: Airline Phase 1-3 Local Git Revision Adoption

Crates:

- `world-env`
- `world-test-lite`
- `world-telemetry`
- `world-identity-core`
- `idempotency-core`
- `delivery-core`
- `http-primitives`
- `rate-limit-core`
- `tenant-scope-sqlx`
- `world-clock-core`

Candidate revision or tag:

- local extraction spike: path dependency from `/home/dean/airline` to
  `/home/dean/world-infra`;
- release-candidate local git proof:
  `file:///home/dean/world-infra?rev=a3882e8cca0bf8a4c38f1c2868e52a6031f70dd5`.

Consumer: Airline.

Owner: Airline platform owner, pending formal review.

Date: 2026-06-28.

Scope:

- `apps/airline-utils` keeps its product-local facade but delegates lenient
  boolean parsing, optional scalar parsing, and duration parsing to `world-env`.
- `apps/airline-utils` tests use `world-test-lite::EnvVarGuard` for
  process-global environment mutation cleanup.
- `apps/loco-app` tracing setup uses `world-telemetry` with `otlp-grpc-tonic`,
  preserving best-effort startup, default `info` filtering, and Airline's
  `OTEL_EXPORTER_OTLP_ENDPOINT` gRPC/tonic exporter posture.
- Airline lockfile was upgraded from
  `opentelemetry`/`opentelemetry-otlp` `0.31` to the shared `0.32` stack as part
  of telemetry adoption.
- `apps/loco-app` exposes a narrow `world_infra` adapter over
  `WorldRef<i32, IncarnationId<Uuid>>`, proving Airline durable-world identity
  cannot omit `world_instance_id`.
- `apps/loco-app` uses `idempotency-core` only for new world-instance-scoped key
  construction. Existing Redis, event, cycle, and delivery key formats are not
  parsed, normalized, or migrated.
- `apps/loco-app` notification delivery retry delay calculation uses
  `delivery-core::BackoffPolicy` while preserving Airline's existing 15s, 30s,
  60s, 120s, 240s, then 10m cap sequence and product-owned delivery row status
  transitions.
- `apps/loco-app` immediate and daily-digest notification delivery claim paths
  wrap product-owned SQL claim rows in `delivery-core::ClaimedDelivery` with the
  existing UUID processing token represented as a shared `LeaseToken`.
- Airline still owns notification delivery SQL, preference filtering, quiet
  hours, tier gates, provider sends, retry-vs-failed policy, and finalization
  row updates.
- `apps/loco-app` notification delivery finalization uses
  `delivery-core::DeliveryFinalizer` and `DeliveryAttemptOutcome` underneath
  the existing sent/retry/failed mark helpers. Ambiguous-after-side-effect
  outcomes are represented and are not auto-finalized.
- `apps/loco-app` request IP extraction delegates trusted proxy CIDR parsing and
  forwarded-header precedence to `http-primitives` while preserving Airline's
  production proxy-header gates and development fallback behavior.
- `apps/loco-app` rate-limit middleware and runtime use `rate-limit-core` for
  in-process and Redis fixed-window backend mechanics.
- Airline still owns route labels, tier labels, account/pre-auth lookup,
  account hashing, Redis fail-open/fail-closed policy, metrics, health response
  shape, and `0`-means-force-closed adapter behavior.
- `apps/loco-app` and `apps/sim-engine` use `tenant-scope-sqlx` to build
  reviewed `SET LOCAL app.* = ...` tenant-scope statements while keeping product
  world and airline scope names local.
- `apps/loco-app` uses `world-clock-core::Cadence` inside its existing
  next-cycle start helper, preserving Airline-owned scheduler authority, lease
  handling, pause controls, overrun catch-up, and `0`-means-immediate adapter
  behavior.
- Airline lockfile was updated from Redis `1.0.2` to `1.2.4`, matching the
  shared backend requirement.

Commands run:

```sh
cargo fmt --check
cargo test -p airline-utils
cargo test -p loco-app observability
cargo test -p loco-app telemetry_config
cargo test -p loco-app world_infra
cargo test -p loco-app utils::net
cargo test -p loco-app db::rls
cargo test -p loco-app rate_limit
cargo test -p loco-app next_cycle_start_time
cargo test -p loco-app notification_delivery_backoff
cargo test -p loco-app notification_delivery
cargo test -p loco-app --test rate_limit_inproc
cargo test -p loco-app-it --test rate_limit_redis --features tc
cargo test -p sim-engine db::rls
cargo clippy -p airline-utils -p loco-app -p sim-engine --all-targets -- -D warnings
cargo tree -p airline-utils -i world-env
cargo tree -p loco-app -i world-telemetry
cargo tree -p loco-app -i opentelemetry
cargo tree -p loco-app -i world-identity-core
cargo tree -p loco-app -i idempotency-core
cargo tree -p loco-app -i delivery-core
cargo tree -p loco-app -i http-primitives
cargo tree -p loco-app -i rate-limit-core
cargo tree -p loco-app -i tenant-scope-sqlx
cargo tree -p sim-engine -i tenant-scope-sqlx
cargo tree -p loco-app -i world-clock-core
make remote-check-touched
make remote-rust-check-pkg P=airline-utils
```

Results:

- `airline-utils`: 19 tests passed.
- `loco-app observability`: 7 focused observability tests passed on the
  exact-revision rerun, including `world-telemetry` on the shared OpenTelemetry
  stack.
- `loco-app telemetry_config`: 2 focused tests passed, proving best-effort
  local tracing without an endpoint and gRPC/tonic OTLP when
  `OTEL_EXPORTER_OTLP_ENDPOINT` is set.
- `loco-app world_infra`: 3 tests passed; 1029 unrelated tests filtered on the
  exact-revision rerun.
- `loco-app utils::net`: 4 tests passed; request IP/proxy behavior covered.
- `loco-app db::rls`: 1 focused tenant-scope statement test passed.
- `loco-app rate_limit`: 45 library tests and the filtered rate-limit integration
  test passed.
- `loco-app next_cycle_start_time`: 3 focused cadence tests passed, including
  zero-interval parity.
- `loco-app notification_delivery_backoff`: 1 focused delivery-backoff parity
  test passed.
- `loco-app notification_delivery`: 9 focused notification delivery tests
  passed, including shared claim lease-token roundtrip, in-memory
  `DeliveryClaimer` adapter coverage, and ambiguous-after-side-effect
  finalization behavior.
- `loco-app --test rate_limit_inproc`: 1 test passed.
- `loco-app-it --test rate_limit_redis --features tc`: 1 test passed.
- `sim-engine db::rls`: 1 focused tenant-scope statement test passed.
- Focused clippy gate passed with `-D warnings`.
- Exact-revision rerun compiled shared crates from
  `file:///home/dean/world-infra?rev=a3882e8cca0bf8a4c38f1c2868e52a6031f70dd5#a3882e8c`.
- Dependency tree confirms direct consumption of the pinned git source for the
  canaried crates, including `world-telemetry`, `delivery-core`, and
  `tenant-scope-sqlx`.
- Dependency tree confirms `loco-app` now consumes OpenTelemetry through
  `world-telemetry` on `opentelemetry` `0.32`.
- `make remote-check-touched` reached ReadyCI affected Rust rules but failed
  before package checks because the remote workspace could not resolve local
  `/world-infra` path dependencies. Failed run ids observed:
  `run_8eb19e8029aca933`, `run_4949986ad37e4f09`,
  `run_424f22839ed8085e`, and `run_e38eb33f6a76c196`.
- `make remote-rust-check-pkg P=airline-utils` failed for the same dependency
  resolution reason in ReadyCI run `run_91015c09409f2548`.

Failures and disposition:

- Initial rustfmt line wrapping differences in `apps/loco-app/src/world_infra.rs`
  were fixed with `cargo fmt`.
- First rate-limit backend wiring treated Airline's `0` override as a one-request
  limit through `LimitSpec`; fixed by preserving `0` as a product-local
  force-closed adapter behavior before calling shared backend mechanics.
- Redis dependency resolution initially failed because Airline's lockfile pinned
  `redis` `1.0.2`; `cargo update -p redis --precise 1.2.4` aligned it with the
  shared backend.
- An initial combined `cargo test -p loco-app utils::net db::rls` invocation was
  invalid because Cargo accepts one test filter; the filters were rerun as
  separate passing commands.
- Initial strict clippy reported a test module before later production items in
  `apps/loco-app/src/db/rls.rs`; the test module was moved to the end of the
  file and the clippy gate passed.
- An initial `cargo test -p loco-app auto_cycle_tests::unit` filter matched no
  tests after compiling; the concrete `next_cycle_start_time` filter was rerun
  and passed.
- ReadyCI remote wrappers do not mount the sibling `/home/dean/world-infra`
  workspace as `/world-infra`. The exact local `file://` git revision proves
  immutable local consumption, but remote canaries still require a canonical
  remote URL for `world-infra`.

Release decision:

- Suitable as local extraction-spike and exact local git revision evidence for
  the canaried crates.
- `delivery-core` evidence covers retry backoff mechanics, shared claim
  envelopes, shared finalization traits, and shared outcome vocabulary for
  Airline notification delivery.
- Not suitable as remote release consumption until the same revision is
  available from a canonical remote URL or tag and Airline runs its remote
  package/touched gates against that remote, immutable source.
