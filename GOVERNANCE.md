# Governance

Lead team: Chairman.

Shared repository DRI: Chairman world-infra DRI.

Required reviewers:

- Chairman DRI for all releases and behavior-affecting changes.
- Airline owner for any crate or behavior Airline will consume.

Both-product approval is required for:

- semver-breaking changes;
- behavior-affecting releases;
- runtime dependency upgrades;
- persisted key format changes;
- delivery semantic changes;
- world identity or incarnation contract changes.

Each product may veto changes that break its consumed invariants. Product
adapters live in product repositories unless a helper has proven product-neutral
shape in both products.

## Committed Crate Ownership Matrix

No crate implementation should start while its owner is `TBD`. The initial
owners below are named so phase 1-3 implementation can proceed as an extraction
spike; release candidates still require both-product approval.

| Crate | Owner | Required Reviewers | First Consumer | Second Consumer Or Deferral Owner |
| --- | --- | --- | --- | --- |
| `world-telemetry` | Chairman world-infra DRI | Chairman DRI, Airline reviewer | Chairman | Airline observability owner |
| `world-env` | Chairman world-infra DRI | Chairman DRI, Airline reviewer | Chairman | Airline platform owner |
| `world-test-lite` | Chairman world-infra DRI | Chairman DRI, Airline reviewer | Chairman | Airline platform owner |
| `world-identity-core` | Chairman world-infra DRI | Chairman DRI, Airline reviewer | Airline likely first | Chairman game DB owner |
| `http-primitives` | Chairman world-infra DRI | Chairman DRI, Airline reviewer | Airline likely first | Chairman API owner |
| `idempotency-core` | Chairman world-infra DRI | Chairman DRI, Airline reviewer | Chairman | Airline platform owner |
| `world-clock-core` | Chairman world-infra DRI | Chairman DRI, Airline reviewer | Chairman optional | Airline cycle owner |
| `rate-limit-core` | Chairman world-infra DRI | Chairman DRI, Airline reviewer | Airline likely first | Chairman API owner |
| `tenant-scope-sqlx` | Chairman world-infra DRI | Chairman DRI, Airline reviewer | Airline likely first | Chairman DB owner |
| `delivery-core` | Chairman world-infra DRI | Chairman DRI, Airline reviewer | Chairman external alerts | Airline notification owner |
| `world-event-core` | Chairman world-infra DRI | Chairman DRI, Airline reviewer | Chairman outbox canary | Airline cycle-event canary |
| `notification-core` | Chairman world-infra DRI | Chairman DRI, Airline reviewer | Airline notification delivery | Chairman external alerts |
| `world-test-containers` | Chairman world-infra DRI | Chairman DRI, Airline reviewer | Chairman DB tests | Airline test platform owner |
| `event-fanout` | Chairman world-infra DRI | Chairman DRI, Airline reviewer | Airline event fanout canary | Chairman worker/event owner |
| `world-cycle-core` | Chairman world-infra DRI | Chairman DRI, Airline reviewer | Chairman cycle canary | Airline cycle owner |

## Boundaries

Keep product policy local: route labels, plan tiers, notification categories,
auth/session semantics, billing policy, cycle phases, finalization policy, and
event payload meanings are not shared API.
