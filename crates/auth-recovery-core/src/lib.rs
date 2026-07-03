//! Product-neutral account recovery token helpers.
//!
//! This crate owns only reusable mechanics: opaque URL-safe recovery-token
//! minting, SHA-256 token hashing, plausibility prefilters, token-state
//! vocabulary, and response-padding calculations. Products still own account
//! lookup, email/SMS delivery, CAPTCHA/Turnstile policy, rate-limit labels,
//! database schemas, and public response bodies.

use std::{error::Error, fmt, ops::RangeInclusive, time::Duration};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::{Rng, RngCore};
use sha2::{Digest, Sha256};

const DEFAULT_TOKEN_BYTES: usize = 32;
const DEFAULT_URL_SAFE_TOKEN_LEN: usize = 43;

/// Recovery helper error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthRecoveryError {
    /// Token input was blank.
    EmptyToken,
    /// Token input was not plausible.
    InvalidToken,
    /// Padding policy is invalid.
    InvalidPaddingPolicy,
}

impl fmt::Display for AuthRecoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyToken => f.write_str("recovery token is empty"),
            Self::InvalidToken => f.write_str("recovery token is invalid"),
            Self::InvalidPaddingPolicy => f.write_str("recovery response padding is invalid"),
        }
    }
}

impl Error for AuthRecoveryError {}

/// Password reset or email verification token state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RecoveryTokenStatus {
    /// No token row exists.
    Missing,
    /// Token exists but is expired, used, revoked, or staged but not live.
    Gone,
    /// Token exists and can be consumed.
    Usable,
}

impl RecoveryTokenStatus {
    /// Return whether public responses should treat this as credential failure.
    #[must_use]
    pub const fn is_credential_miss(self) -> bool {
        matches!(self, Self::Missing | Self::Gone)
    }
}

/// Result of consuming a one-time recovery token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ConsumeRecoveryTokenResult {
    /// Token did not exist.
    Missing,
    /// Token was expired, used, revoked, or otherwise not usable.
    Gone,
    /// Token was consumed.
    Consumed,
}

impl ConsumeRecoveryTokenResult {
    /// Return whether public responses should treat this as credential failure.
    #[must_use]
    pub const fn is_credential_miss(self) -> bool {
        matches!(self, Self::Missing | Self::Gone)
    }
}

/// Options for recovery-token plausibility checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryTokenPolicy {
    /// Accept UUID-form tokens.
    pub allow_uuid: bool,
    /// Accept URL-safe no-padding base64-like tokens.
    pub allow_url_safe: bool,
    /// Required URL-safe token length when present.
    pub url_safe_len: usize,
}

impl Default for RecoveryTokenPolicy {
    fn default() -> Self {
        Self {
            allow_uuid: true,
            allow_url_safe: true,
            url_safe_len: DEFAULT_URL_SAFE_TOKEN_LEN,
        }
    }
}

/// Mint a new URL-safe no-padding recovery token.
#[must_use]
pub fn mint_recovery_token() -> String {
    let mut bytes = [0_u8; DEFAULT_TOKEN_BYTES];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Return the SHA-256 hex hash of a recovery token.
///
/// Products should store hashes, not raw recovery tokens.
#[must_use]
pub fn hash_recovery_token_sha256(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

/// Validate that a submitted token has a plausible public shape.
///
/// This is a prefilter only. Products must still check the hashed token in
/// their authoritative store.
///
/// # Errors
///
/// Returns [`AuthRecoveryError`] for blank, padded, control-character, or
/// implausible tokens.
pub fn validate_recovery_token(
    token: &str,
    policy: RecoveryTokenPolicy,
) -> Result<&str, AuthRecoveryError> {
    if token.trim() != token {
        return Err(AuthRecoveryError::InvalidToken);
    }
    if token.is_empty() {
        return Err(AuthRecoveryError::EmptyToken);
    }
    if token.chars().any(char::is_control) {
        return Err(AuthRecoveryError::InvalidToken);
    }
    if policy.allow_uuid && is_uuid_like(token) {
        return Ok(token);
    }
    if policy.allow_url_safe
        && token.len() == policy.url_safe_len
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Ok(token);
    }
    Err(AuthRecoveryError::InvalidToken)
}

/// Return whether a token has a plausible public shape.
#[must_use]
pub fn plausible_recovery_token(token: &str, policy: RecoveryTokenPolicy) -> bool {
    validate_recovery_token(token, policy).is_ok()
}

/// Response-padding policy for anti-enumeration endpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponsePaddingPolicy {
    base: Duration,
    jitter: RangeInclusive<u64>,
}

impl ResponsePaddingPolicy {
    /// Construct a response-padding policy with jitter in milliseconds.
    ///
    /// # Errors
    ///
    /// Returns [`AuthRecoveryError`] when the jitter range is inverted.
    pub fn new(base: Duration, jitter_ms: RangeInclusive<u64>) -> Result<Self, AuthRecoveryError> {
        if *jitter_ms.start() > *jitter_ms.end() {
            return Err(AuthRecoveryError::InvalidPaddingPolicy);
        }
        Ok(Self {
            base,
            jitter: jitter_ms,
        })
    }

    /// Construct a response-padding policy from inclusive millisecond bounds.
    ///
    /// # Errors
    ///
    /// Returns [`AuthRecoveryError`] when the upper bound is lower than the
    /// lower bound.
    pub fn from_millis_bounds(min_ms: u64, max_ms: u64) -> Result<Self, AuthRecoveryError> {
        if max_ms < min_ms {
            return Err(AuthRecoveryError::InvalidPaddingPolicy);
        }
        Self::new(Duration::from_millis(min_ms), 0..=(max_ms - min_ms))
    }

    /// Pick a target response duration.
    #[must_use]
    pub fn target_duration(&self) -> Duration {
        let jitter_ms = rand::thread_rng().gen_range(self.jitter.clone());
        self.base.saturating_add(Duration::from_millis(jitter_ms))
    }

    /// Return how long remains to reach `target` after `elapsed`.
    #[must_use]
    pub fn remaining_delay(target: Duration, elapsed: Duration) -> Duration {
        target.saturating_sub(elapsed)
    }
}

fn is_uuid_like(token: &str) -> bool {
    let bytes = token.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (idx, byte) in bytes.iter().copied().enumerate() {
        if matches!(idx, 8 | 13 | 18 | 23) {
            if byte != b'-' {
                return false;
            }
        } else if !byte.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minted_tokens_are_url_safe_and_hashable() {
        let token = mint_recovery_token();
        assert_eq!(token.len(), 43);
        assert!(plausible_recovery_token(
            &token,
            RecoveryTokenPolicy::default()
        ));
        let hash = hash_recovery_token_sha256(&token);
        assert_eq!(hash.len(), 64);
        assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn plausible_tokens_accept_uuid_and_reject_unsafe_shapes() {
        let policy = RecoveryTokenPolicy::default();
        assert!(plausible_recovery_token(
            "550e8400-e29b-41d4-a716-446655440000",
            policy
        ));
        assert!(!plausible_recovery_token("", policy));
        assert!(!plausible_recovery_token(" token", policy));
        assert!(!plausible_recovery_token("abc/def", policy));
        assert!(!plausible_recovery_token("abc\n", policy));
    }

    #[test]
    fn status_and_consume_results_classify_public_misses() {
        assert!(RecoveryTokenStatus::Missing.is_credential_miss());
        assert!(RecoveryTokenStatus::Gone.is_credential_miss());
        assert!(!RecoveryTokenStatus::Usable.is_credential_miss());
        assert!(ConsumeRecoveryTokenResult::Missing.is_credential_miss());
        assert!(ConsumeRecoveryTokenResult::Gone.is_credential_miss());
        assert!(!ConsumeRecoveryTokenResult::Consumed.is_credential_miss());
    }

    #[test]
    fn response_padding_calculates_remaining_delay() {
        let policy = ResponsePaddingPolicy::from_millis_bounds(180, 320).unwrap();
        let target = policy.target_duration();
        let second_target = policy.target_duration();
        assert!(target >= Duration::from_millis(180));
        assert!(target <= Duration::from_millis(320));
        assert!(second_target >= Duration::from_millis(180));
        assert!(second_target <= Duration::from_millis(320));
        assert_eq!(
            ResponsePaddingPolicy::remaining_delay(
                Duration::from_millis(250),
                Duration::from_millis(300)
            ),
            Duration::ZERO
        );
    }

    #[test]
    fn response_padding_rejects_inverted_bounds() {
        assert_eq!(
            ResponsePaddingPolicy::from_millis_bounds(320, 180),
            Err(AuthRecoveryError::InvalidPaddingPolicy)
        );
    }

    #[test]
    fn response_padding_saturates_extreme_config_values() {
        let policy = ResponsePaddingPolicy::new(Duration::MAX, 1..=1).unwrap();
        assert_eq!(policy.target_duration(), Duration::MAX);
    }
}
