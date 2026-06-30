# RFC: idempotency-runtime-core

## Status

Approved

## Summary

Create `idempotency-runtime-core` for volatile service-edge idempotency claim
stores. The crate owns in-memory and Redis pending/completed claim mechanics,
namespaced runtime keys, timeout handling, marker semantics, and unsupported
marker detection.

This supersedes the Phase 7 and Phase 8 candidate-audit deferral for runtime
idempotency backends. The earlier deferral was correct while only one mature
runtime implementation was proven; the post-plan extraction is limited to the
small runtime contract requested for Airline adoption and kept independent of
product SQL ledgers.

## Product Evidence

- Airline adapter/example:
  `/home/dean/airline/apps/loco-app/src/idempotency.rs` has mature
  in-memory/Redis pending and completed claim behavior, namespaces, timeouts,
  and marker handling. Airline remains the first expected consumer.
- Chairman adapter/example:
  Chairman's durable action idempotency is currently SQL/request-led, so it is
  not required to adopt this runtime crate before release. Chairman services may
  use the volatile store later for edge work that matches the same
  pending/completed claim semantics.
- Approved deferral:
  Chairman adoption is deferred. The crate may exist because its API is
  product-neutral, does not own persisted keys, and has an explicit Airline
  adoption path. Release candidates still require the normal Chairman DRI and
  Airline reviewer approval.

## Boundaries

Product-owned:

- persisted SQL idempotency ledgers;
- canonical request hashing and semantic request normalization;
- durable key migrations or persisted-key parsing;
- HTTP response shapes for pending/completed/conflict outcomes;
- world, account, tenant, and incarnation authorization;
- API route labels and namespace names.

Shared:

- `WorkClaim::{Fresh, Pending, Completed}`;
- validated runtime namespace and key construction;
- in-memory TTL-backed claim storage;
- Redis pending/completed/marker storage behind an optional feature;
- command timeout and backend error vocabulary;
- unsupported-marker detection for corrupted or incompatible runtime keys.

## API Sketch

```rust
let namespace = IdempotencyNamespace::new("airline:api")?;
let store = InMemoryIdempotencyStore::with_namespace(namespace);

match store.claim_pending("request-key", ttl).await? {
    WorkClaim::Fresh => {
        // Product performs work, then marks completion.
    }
    WorkClaim::Pending => {
        // Product maps to its local pending response.
    }
    WorkClaim::Completed => {
        // Product maps to its local replay/completed response.
    }
}
```

Redis support remains optional:

```rust
#[cfg(feature = "redis")]
let store = RedisIdempotencyStore::new(redis_client, command_timeout)
    .with_namespace(namespace);
```

## Compatibility

- Semver: additive new crate.
- Persisted keys: none. Runtime Redis keys are volatile and length-prefixed to
  avoid namespace/key delimiter ambiguity.
- Wire format: none.
- SQL: none.
- Runtime dependencies: Redis and Tokio are feature-gated.
- Rollback: products can keep or restore their local idempotency runtime
  implementations without data migration. Existing persisted SQL idempotency
  ledgers are unaffected.

## Verification

Shared tests:

- in-memory `set_once` has a single winner;
- pending claims return `Fresh` then `Pending`;
- completed and marker entries return `Completed`;
- TTL expiry permits fresh claims after expiration;
- namespace and key validation reject unsafe values;
- Redis feature tests cover pending/completed claims, marker handling, TTL
  rounding, namespace isolation, and unsupported markers when
  `IDEMPOTENCY_REDIS_URL` or `WORLD_INFRA_REDIS_URL` is present.

Consumer canaries:

- Airline should replace its local runtime store behind existing
  idempotency-route tests before relying on a release candidate.
- Chairman adoption is not required for the first release; any future Chairman
  use must be service-edge and must not replace durable SQL action ledgers.
