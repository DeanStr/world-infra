# Dependency Compatibility

Current source-product inventory was captured from `/home/dean/chairman` and
`/home/dean/airline`.

| Dependency | Chairman | Airline | Shared Resolution |
| --- | --- | --- | --- |
| `sqlx` | `0.8` | `0.8` | `0.8`, optional `sqlx-postgres` feature |
| `redis` | `1.2` | `1.2.4` lockfile after Airline canary | `1.2`, optional `redis` feature |
| `axum` | `0.8` | `0.8` | no core dependency; adapters only later |
| `tower-http` | `0.6` | `0.6` | no core dependency; adapters only later |
| `tracing` | `0.1` | `0.1` | `0.1` |
| `opentelemetry` | `0.32` | `0.32` after Airline canary | `0.32` through `world-telemetry` |
| `opentelemetry-otlp` | `0.32` HTTP/protobuf | `0.32` gRPC/tonic after Airline canary | shared crate supports HTTP/protobuf and gRPC/tonic features |
| `reqwest` | `0.12` | `0.13` | no shared runtime dependency in phases 0-3 |
| `serde` | `1.0` | `1` | `1.0`, optional where possible |
| `time` | not broad workspace dependency | `0.3` | `0.3`, optional where needed |

## Resolution Notes

- Shared crates use Rust edition 2021 with MSRV 1.88 because current `redis`
  and `tonic` integration dependencies require Rust 1.88.
- The original OpenTelemetry skew was resolved in the Airline local-path canary
  and rerun against the `world-infra-v0.1.0-rc.3` remote candidate:
  `loco-app` now consumes `world-telemetry` with `otlp-grpc-tonic`, upgrading
  `opentelemetry`/`opentelemetry-otlp` from `0.31` to `0.32` while preserving
  Airline's gRPC/tonic exporter behavior.
- `reqwest` is intentionally absent from first-wave shared APIs to avoid forcing
  the Chairman/Airline version gap into unrelated crates.
- SQLx and Redis are optional integrations. Pure crates must compile without
  them.
- Airline `rate-limit-core` canary required `cargo update -p redis --precise
  1.2.4` because the previous Airline lockfile selected `redis` `1.0.2` while
  the shared Redis backend requires `1.2`.
