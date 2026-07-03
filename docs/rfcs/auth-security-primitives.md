# RFC: auth security primitives

## Status

Approved

## Summary

Create a small auth/security lane for product-neutral authentication mechanics:
claim vocabulary, strict bearer parsing, impersonation claim-shape validation,
password hashing and password-policy helpers, opaque refresh-token
minting/rotation stores, refresh-cookie/origin helpers, and WebSocket
auth-frame primitives. The same lane also covers product-neutral device/session
metadata helpers and recovery-token mechanics, without owning account recovery
or trusted-device product policy.

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
- bounded device-id/session-label helpers and optional HMAC/SHA-256 device
  hash construction;
- token type, issuer/audience, claim-name constants, actor id, and
  session-version/freshness value helpers;
- impersonation actor-id / actor-session-version shape validation;
- Argon2 password hashing/verification helpers;
- configurable password length/common/breach-file validation primitives;
- opaque refresh-token minting, hashing, TTL clamping, single-use rotation, and
  optional Redis backend mechanics;
- configurable refresh Redis key version and hash-field names;
- refresh lookup disposition helpers so invalid token input can map to generic
  credential-miss responses without hiding internal errors from adapters;
- invalid stored refresh-payload discard disposition helpers; products still
  own deletion, audit, metrics, and response text;
- refresh-cookie string construction and trusted-origin exact matching;
- recovery-token minting, plausibility prefilters, SHA-256 hashing,
  Missing/Gone/Usable vocabulary, and response-padding calculations;
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
let label = auth_device_core::SessionLabel::optional_or_default(
    browser_label,
    "Web browser",
)?;

let device_hash_policy = auth_device_core::DeviceHashPolicy::new()
    .with_domain_separator("product-device-hash:v1")?
    .with_device_id_normalization(auth_device_core::DeviceIdNormalization::Trimmed {
        max_bytes: 128,
        reject_control: false,
    })
    .with_fallback_normalization(auth_device_core::FallbackMaterialNormalization::RawPresent)
    .with_unkeyed_device_id_hash(auth_device_core::UnkeyedDeviceIdHash::RawSha256)
    .with_unkeyed_ip_user_agent_hash(
        auth_device_core::UnkeyedIpUserAgentHash::LegacyLenPrefixed,
    );

// Products with existing device-hash evidence should preserve both the
// device-id and fallback IP/user-agent hash modes during adoption.
// The legacy IP/user-agent fallback mode intentionally requires both fields to
// avoid ambiguous one-field hashes; products with historical one-field fallback
// rows should handle those through product-owned migration/dual-read logic.

let hash = auth_device_core::compute_device_hash_with_policy(
    auth_device_core::DeviceHashInput::new()
        .with_device_id(device_id)
        .with_user_agent(user_agent)
        .with_ip(client_ip),
    device_hash_key.as_ref(),
    &device_hash_policy,
);
```

```rust
let token = auth_recovery_core::mint_recovery_token();
let token_hash = auth_recovery_core::hash_recovery_token_sha256(&token);
assert!(auth_recovery_core::plausible_recovery_token(
    &token,
    auth_recovery_core::RecoveryTokenPolicy::default(),
));
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
        // Return the same product-owned unauthenticated response as an absent
        // or expired refresh token.
    }
    Err(error) if error.should_treat_as_credential_miss() => {
        // Return the same product-owned unauthenticated response as an absent
        // or expired refresh token.
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
- device-id/session-label bounds, HMAC/SHA-256 device hash construction, and
  unsafe device metadata rejection;
- recovery-token mint/hash/plausibility, Missing/Gone/Usable vocabulary, and
  response-padding calculations;
- Argon2 hash/verify and async wrappers;
- claim-name constants, actor id validation, impersonation actor-field
  consistency, actor-session freshness wrappers;
- refresh-token mint/hash, namespace validation, TTL clamping, set/get/delete,
  rotate/replay, same-token rotation rejection, configurable key/field schemas,
  lookup disposition, and invalid-stored-payload discard disposition;
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
