# RFC: auth security primitives

## Status

Approved

## Summary

Create a small auth/security lane for product-neutral authentication mechanics:
claim vocabulary, strict bearer parsing, impersonation claim-shape validation,
password hashing and password-policy helpers, opaque refresh-token
minting/rotation stores, refresh-cookie/origin helpers, and WebSocket
auth-frame primitives.

This supersedes the Phase 8 audit's broad auth deferral only for these narrow
mechanics. It does not approve a shared auth service, shared login UX, shared
account tables, shared role policy, shared entitlement logic, or shared token
signing/verification policy.

## Product Evidence

- Chairman adapter/example:
  Chairman is implementing real login/session UX and can use these primitives
  for password hashing, refresh-cookie construction, bearer parsing,
  WebSocket auth-frame validation, and refresh-token rotation without coupling
  the root simulation engine to game/account concepts.
- Airline adapter/example:
  Airline already has the mature source patterns for password length/common
  checks, breach-file validation, opaque refresh tokens, Redis refresh stores,
  trusted origin checks, and WebSocket auth hygiene. Airline may adopt these
  selectively where shared behavior is equivalent.
- Approved deferral:
  Airline refresh-store replacement is deferred until compatibility is proven
  with its `rt:v3` key shape, `account_id` / `issued_at` field names, Redis
  behavior tests, and existing auth-route coverage. Airline password-policy
  replacement is deferred unless the shared policy is configured to preserve
  Airline's `12..=256` length and common-password normalization behavior.

## Boundaries

Product-owned:

- account records and persistence schema;
- signup/login/logout routes and response bodies;
- password reset, email verification, captcha, MFA, and account recovery;
- JWT signing keys, token verification, issuer rotation, and key storage;
- role checks, account tiers, admin policy, impersonation authorization, and
  entitlements;
- session lifecycle policy, device binding, fetch-metadata policy, CSRF
  strategy, and cookie SameSite decisions that depend on deployment topology;
- WebSocket subscription authorization and product wire payload schemas.

Shared:

- strict bearer-header parsing and safe token redaction;
- token type, issuer/audience, claim-name constants, actor id, and
  session-version/freshness value helpers;
- impersonation actor-id / actor-session-version shape validation;
- Argon2 password hashing/verification helpers;
- configurable password length/common/breach-file validation primitives;
- opaque refresh-token minting, hashing, TTL clamping, single-use rotation, and
  optional Redis backend mechanics;
- configurable refresh Redis key version and hash-field names;
- invalid stored refresh-payload discard disposition helpers; products still
  own deletion, audit, metrics, and response text;
- refresh-cookie string construction and trusted-origin exact matching;
- initial WebSocket auth-frame parsing, auth nonce validation, and small
  control-frame rate-limit vocabulary.

## API Sketch

```rust
let policy = PasswordPolicy {
    min_chars: 12,
    max_chars: 256,
    common_password_normalization:
        CommonPasswordNormalization::AsciiAlphanumericLowercase,
    ..PasswordPolicy::default()
};
policy.validate(password)?;
```

```rust
let config = RefreshStoreConfig::default()
    .with_key_version("v3")?
    .with_session_fields("account_id", "session_id", "issued_at", "session_version")?;

let store = RedisRefreshStore::new(redis_client, command_timeout)
    .with_namespace(RefreshNamespace::new("product")?)
    .with_config(config);
```

```rust
let shape = ImpersonationClaimShape::new(
    TokenType::Impersonation,
    Some(ActorId::new(actor_id)?),
    Some(actor_session_version),
);
assert!(shape.is_consistent());
```

```rust
match store.get(refresh_token).await {
    Err(error) if error.should_discard_stored_payload() => {
        store.delete(refresh_token).await?;
        // Force product-owned re-authentication response.
    }
    result => { /* product-owned handling */ }
}
```

## Compatibility

- Semver: additive new crates plus additive API in `auth-primitives`.
- Persisted keys: refresh-store defaults use `rt:v1`; products with existing
  volatile keys must configure their current key version/field names or keep
  local adapters until a re-login migration is acceptable.
- Wire format: no shared public API response format. WebSocket helpers parse an
  initial auth frame but products own emitted frame schemas and error payloads.
- Impersonation: shared helpers only validate claim shape. Products own who may
  impersonate, maximum duration, actor audit trails, UI warnings, and
  permission restrictions.
- SQL: none.
- Security: shared errors and logs must not include raw bearer, refresh, or
  provider tokens.
- Rollback: products can keep or restore their local auth implementations. The
  crates do not own account data or durable auth migrations.

## Verification

Shared tests:

- password length, common-password, product-supplied denylist, and malformed
  breach-file handling;
- Argon2 hash/verify and async wrappers;
- claim-name constants, actor id validation, impersonation actor-field
  consistency, actor-session freshness wrappers;
- refresh-token mint/hash, namespace validation, TTL clamping, set/get/delete,
  rotate/replay, same-token rotation rejection, configurable key/field schemas,
  and invalid-stored-payload discard disposition;
- Redis refresh backend coverage for set/get/delete, rotate/replay, expiry,
  bad stored payload deletion, namespace isolation, and configured `rt:v3`
  field names when `AUTH_REFRESH_REDIS_URL` or `WORLD_INFRA_REDIS_URL` is set;
- cookie value/domain/header-injection validation and `__Host-` constraints;
- trusted origin/referrer exact matching and malformed authority rejection;
- WebSocket auth-frame size/type/token validation, nonce checks, and
  control-frame limiting.

Consumer canaries:

- Chairman should adopt these in the real-login and WebSocket-auth hardening
  work while preserving local roles, club/world authorization, and API
  response bodies.
- Airline should first adopt `auth-primitives` and may selectively use
  `auth-http-core` / `auth-ws-core`. Replacing Airline password policy or
  refresh storage requires product tests proving behavior-equivalent length,
  common-password, breach-file, key-schema, Redis, and route semantics.
