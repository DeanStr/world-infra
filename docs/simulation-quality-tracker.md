# Simulation Quality Tracker

This lane is separate from `world-infra`. It covers reusable validation,
calibration, deterministic fixture, and repository-hygiene ideas that may be
useful across simulation products.

Potential candidates:

- `simulation-validation-core`;
- `simulation-calibration-core`;
- `repo-hygiene-test-support`;
- `deterministic-fixture-core`;
- `idempotency-request-core`;
- `worker-ops-core`.

Do not create these crates in `world-infra` without a separate design note and
both-product approval.
