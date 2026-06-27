# Shared Crate Readiness Checklist

Before product adoption, each crate must have:

- source behavior characterized in the source product;
- product-neutral tests;
- public API docs and at least one product-neutral example;
- feature combinations tested:
  - default features;
  - `--no-default-features`;
  - each optional feature independently where practical;
  - `--all-features`;
- runtime-heavy features disabled by default unless both products approve;
- rustdoc examples for public APIs;
- checked-in CI coverage for `cargo-hack` feature powersets where practical;
- checked-in dependency audit, license policy, and secret scanning;
- release-candidate `cargo-semver-checks` where a baseline exists;
- stable fixture tests for serialized forms, keys, queue payloads, and
  log-boundary values;
- typed library errors where practical;
- product adapter mapping plan for API/HTTP shapes;
- rollback or downgrade notes for each product adoption PR;
- ownership and release approvers recorded.

Security/privacy review is required for HTTP, telemetry, IP/header parsing,
identity, rate limits, delivery, events, notifications, persistence,
auth-adjacent metadata, or commercial risk.
