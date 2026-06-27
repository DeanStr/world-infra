# Supply Chain Policy

## Licenses

Allowed licenses:

- MIT
- Apache-2.0
- BSD-2-Clause
- BSD-3-Clause
- ISC
- Unicode-3.0

Other licenses require explicit review before use.

## Dependency Audit

Release candidates must run a dependency audit such as `cargo-audit` or
`cargo-deny`. Yanked or vulnerable dependencies block release unless both
products approve a documented temporary exception with owner and due date.

The checked-in CI workflow runs `cargo audit` and `cargo deny check` for release
candidate readiness.

## Secrets

Shared repository CI must include secret scanning before production release
tags. No bearer tokens, cookies, refresh tokens, push auth keys, provider
secrets, SMTP credentials, or API credentials may appear in tests, fixtures,
logs, panics, or documentation examples.

The checked-in CI workflow runs Gitleaks secret scanning.

## Provenance

Published crates and tags must be immutable. Product branches consume tags or
exact revisions, never a moving shared branch.

Release candidates should run `cargo semver-checks` where a baseline revision
exists. The initial repository bootstrap records the command in CI as a
non-blocking first-run check because no previous release baseline exists yet.
