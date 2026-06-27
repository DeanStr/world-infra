# Releasing

## Tags

Use crate-scoped tags for independent releases, for example:

- `world-telemetry-v0.1.0`
- `world-env-v0.1.0`

Use `world-infra-vX.Y.Z` only for a coordinated multi-crate release.

## Changelog

Every release entry must include:

- API additions;
- behavior changes;
- dependency changes;
- migration notes;
- required product verification;
- rollback or downgrade steps for each adopting product.

## Consumer Upgrade Rule

Committed product work must pin shared crates to immutable git tags or exact
revisions. Path dependencies are allowed only for local development and
extraction spikes.

Use `scripts/pinned-product-deps.sh` to generate exact-revision dependency
entries for Chairman and Airline canaries. The helper intentionally fails before
the shared repository has a commit, because an uncommitted workspace cannot be a
release candidate.

Each shared release requires:

1. shared CI green;
2. release-candidate consumer canary for Chairman;
3. release-candidate consumer canary for Airline or an approved dated deferral;
4. both required reviewers recorded for release candidates and final tags.

## Breaking Changes

Breaking changes require a design note, migration steps, both-product approval,
and fixture-backed tests when persisted keys, SQL behavior, wire contracts, queue
payloads, or delivery semantics are affected.
