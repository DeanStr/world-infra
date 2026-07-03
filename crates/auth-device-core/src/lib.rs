//! Product-neutral device identity and session-label helpers.
//!
//! This crate owns only small mechanics: bounded device ids, user-facing
//! session labels, and optional HMAC/SHA-256 device hash construction. Products
//! still own account schemas, trusted-device policy, IP retention policy,
//! login alerts, and UI copy.

use std::{error::Error, fmt};

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

const DEFAULT_DEVICE_ID_MAX_CHARS: usize = 128;
const DEFAULT_SESSION_LABEL_MAX_CHARS: usize = 80;
const DEFAULT_KEY_ID: &str = "v1";
const DEFAULT_DOMAIN_SEPARATOR: &str = "auth-device-core:v1";

/// Error returned by device/session helpers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthDeviceError {
    /// Value was blank after trimming.
    Empty {
        /// Field name.
        field: &'static str,
    },
    /// Value contained control characters, unsafe formatting, or bad length.
    Invalid {
        /// Field name.
        field: &'static str,
    },
}

impl fmt::Display for AuthDeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { field } => write!(f, "{field} is empty"),
            Self::Invalid { field } => write!(f, "{field} is invalid"),
        }
    }
}

impl Error for AuthDeviceError {}

/// Bounded client-provided device id.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(String);

impl DeviceId {
    /// Validate a device id using the default 128-character maximum.
    ///
    /// # Errors
    ///
    /// Returns [`AuthDeviceError`] for blank, padded, control-character, or
    /// oversized values.
    pub fn new(value: impl AsRef<str>) -> Result<Self, AuthDeviceError> {
        Self::with_max_chars(value, DEFAULT_DEVICE_ID_MAX_CHARS)
    }

    /// Validate a device id with a product-supplied character limit.
    ///
    /// # Errors
    ///
    /// Returns [`AuthDeviceError`] for blank, padded, control-character, or
    /// oversized values.
    pub fn with_max_chars(
        value: impl AsRef<str>,
        max_chars: usize,
    ) -> Result<Self, AuthDeviceError> {
        let value = validate_visible_trimmed(value.as_ref(), "device_id", max_chars)?;
        Ok(Self(value.to_owned()))
    }

    /// Access the normalized device id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// User-facing session/device label.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionLabel(String);

impl SessionLabel {
    /// Validate a label using the default 80-character maximum.
    ///
    /// # Errors
    ///
    /// Returns [`AuthDeviceError`] for blank, padded, control-character, or
    /// oversized values.
    pub fn new(value: impl AsRef<str>) -> Result<Self, AuthDeviceError> {
        Self::with_max_chars(value, DEFAULT_SESSION_LABEL_MAX_CHARS)
    }

    /// Validate a label with a product-supplied character limit.
    ///
    /// # Errors
    ///
    /// Returns [`AuthDeviceError`] for blank, padded, control-character, or
    /// oversized values.
    pub fn with_max_chars(
        value: impl AsRef<str>,
        max_chars: usize,
    ) -> Result<Self, AuthDeviceError> {
        let value = validate_visible_trimmed(value.as_ref(), "session_label", max_chars)?;
        Ok(Self(value.to_owned()))
    }

    /// Normalize an optional label, falling back when input is absent or invalid.
    ///
    /// This is useful for login flows where a browser-provided label improves
    /// UX but should never reject a valid sign-in.
    pub fn optional_or_default(
        value: Option<&str>,
        fallback: &str,
    ) -> Result<Self, AuthDeviceError> {
        value
            .and_then(|value| Self::new(value).ok())
            .or_else(|| Self::new(fallback).ok())
            .ok_or(AuthDeviceError::Invalid {
                field: "session_label",
            })
    }

    /// Access the normalized label.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Inputs used to build a stable best-effort device hash.
#[derive(Debug, Clone, Copy, Default)]
pub struct DeviceHashInput<'a> {
    /// Preferred stable client-generated device id.
    pub device_id: Option<&'a str>,
    /// Request user-agent string, used only when no device id is present.
    pub user_agent: Option<&'a str>,
    /// Product-normalized client IP, used only when no device id is present.
    pub ip: Option<&'a str>,
}

impl<'a> DeviceHashInput<'a> {
    /// Construct empty hash input.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            device_id: None,
            user_agent: None,
            ip: None,
        }
    }

    /// Set the device id.
    #[must_use]
    pub const fn with_device_id(mut self, device_id: Option<&'a str>) -> Self {
        self.device_id = device_id;
        self
    }

    /// Set user-agent text.
    #[must_use]
    pub const fn with_user_agent(mut self, user_agent: Option<&'a str>) -> Self {
        self.user_agent = user_agent;
        self
    }

    /// Set client IP text.
    #[must_use]
    pub const fn with_ip(mut self, ip: Option<&'a str>) -> Self {
        self.ip = ip;
        self
    }
}

/// Optional HMAC key for device hashes.
#[derive(Clone, PartialEq, Eq)]
pub struct DeviceHashKey {
    key_id: String,
    secret: Vec<u8>,
}

impl fmt::Debug for DeviceHashKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceHashKey")
            .field("key_id", &self.key_id)
            .field("secret", &"[redacted]")
            .finish()
    }
}

impl DeviceHashKey {
    /// Construct a key using key id `v1`.
    ///
    /// # Errors
    ///
    /// Returns [`AuthDeviceError`] when the secret is blank.
    pub fn new(secret: impl AsRef<[u8]>) -> Result<Self, AuthDeviceError> {
        Self::with_key_id(DEFAULT_KEY_ID, secret)
    }

    /// Construct a key with a product-owned key id.
    ///
    /// # Errors
    ///
    /// Returns [`AuthDeviceError`] when the key id or secret is unsafe.
    pub fn with_key_id(
        key_id: impl AsRef<str>,
        secret: impl AsRef<[u8]>,
    ) -> Result<Self, AuthDeviceError> {
        let key_id = validate_key_id(key_id.as_ref())?;
        let secret = secret.as_ref();
        if secret.is_empty() {
            return Err(AuthDeviceError::Empty { field: "secret" });
        }
        Ok(Self {
            key_id: key_id.to_owned(),
            secret: secret.to_vec(),
        })
    }

    /// Construct a key using a product-supplied key id when valid, or `v1`.
    ///
    /// This matches common configuration behavior where an unsafe or blank key id
    /// should not disable login/session flows.
    ///
    /// # Errors
    ///
    /// Returns [`AuthDeviceError`] when the secret is blank.
    pub fn with_key_id_or_default(
        key_id: Option<&str>,
        secret: impl AsRef<[u8]>,
    ) -> Result<Self, AuthDeviceError> {
        let key_id = key_id
            .map(str::trim)
            .and_then(|value| validate_key_id(value).ok())
            .unwrap_or(DEFAULT_KEY_ID);
        Self::with_key_id(key_id, secret)
    }

    /// Access the public key id prefix.
    #[must_use]
    pub fn key_id(&self) -> &str {
        &self.key_id
    }
}

/// Device-id normalization strategy for device-hash material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceIdNormalization {
    /// Require the strict [`DeviceId`] shape.
    StrictVisible,
    /// Trim input and reject only blank, oversized, and optionally control input.
    Trimmed {
        /// Maximum byte length after trimming.
        max_bytes: usize,
        /// Whether to reject control characters after trimming.
        reject_control: bool,
    },
}

/// IP/user-agent fallback normalization strategy for device hashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackMaterialNormalization {
    /// Trim, bound, and reject control characters.
    TrimmedVisible {
        /// Maximum character length for IP text.
        ip_max_chars: usize,
        /// Maximum character length for user-agent text.
        user_agent_max_chars: usize,
    },
    /// Use present values exactly as supplied.
    RawPresent,
}

/// Unkeyed device-id hashing strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnkeyedDeviceIdHash {
    /// Hash the same domain-separated, length-prefixed material used by HMAC.
    DomainSeparated,
    /// Hash only the normalized device-id bytes.
    RawSha256,
}

/// Unkeyed IP/user-agent fallback hashing strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnkeyedIpUserAgentHash {
    /// Hash the same domain-separated, length-prefixed material used by HMAC.
    DomainSeparated,
    /// Hash only length-prefixed IP/user-agent values without labels or domain.
    ///
    /// This preserves legacy two-field fallback hashes. To avoid ambiguous
    /// one-field hashes, this mode returns no hash unless both IP and user-agent
    /// material are present.
    LegacyLenPrefixed,
}

/// Policy for constructing stable device hashes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceHashPolicy {
    domain_separator: String,
    device_id_normalization: DeviceIdNormalization,
    fallback_normalization: FallbackMaterialNormalization,
    unkeyed_device_id_hash: UnkeyedDeviceIdHash,
    unkeyed_ip_user_agent_hash: UnkeyedIpUserAgentHash,
}

impl Default for DeviceHashPolicy {
    fn default() -> Self {
        Self {
            domain_separator: DEFAULT_DOMAIN_SEPARATOR.to_owned(),
            device_id_normalization: DeviceIdNormalization::StrictVisible,
            fallback_normalization: FallbackMaterialNormalization::TrimmedVisible {
                ip_max_chars: DEFAULT_DEVICE_ID_MAX_CHARS,
                user_agent_max_chars: 512,
            },
            unkeyed_device_id_hash: UnkeyedDeviceIdHash::DomainSeparated,
            unkeyed_ip_user_agent_hash: UnkeyedIpUserAgentHash::DomainSeparated,
        }
    }
}

impl DeviceHashPolicy {
    /// Construct the default policy.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the product-owned hash domain separator.
    ///
    /// Pass values without the trailing section separator, for example
    /// `auth-device-core:v1`.
    ///
    /// # Errors
    ///
    /// Returns [`AuthDeviceError`] when the separator is blank, padded,
    /// oversized, contains control characters, or ends with `:`.
    pub fn with_domain_separator(
        mut self,
        domain_separator: impl AsRef<str>,
    ) -> Result<Self, AuthDeviceError> {
        let domain_separator = validate_visible_trimmed(
            domain_separator.as_ref(),
            "domain_separator",
            DEFAULT_DEVICE_ID_MAX_CHARS,
        )?;
        if domain_separator.ends_with(':') {
            return Err(AuthDeviceError::Invalid {
                field: "domain_separator",
            });
        }
        self.domain_separator = domain_separator.to_owned();
        Ok(self)
    }

    /// Set device-id normalization.
    #[must_use]
    pub const fn with_device_id_normalization(
        mut self,
        normalization: DeviceIdNormalization,
    ) -> Self {
        self.device_id_normalization = normalization;
        self
    }

    /// Set IP/user-agent fallback normalization.
    #[must_use]
    pub const fn with_fallback_normalization(
        mut self,
        normalization: FallbackMaterialNormalization,
    ) -> Self {
        self.fallback_normalization = normalization;
        self
    }

    /// Set unkeyed device-id hashing behavior.
    #[must_use]
    pub const fn with_unkeyed_device_id_hash(mut self, hash: UnkeyedDeviceIdHash) -> Self {
        self.unkeyed_device_id_hash = hash;
        self
    }

    /// Set unkeyed IP/user-agent hashing behavior.
    #[must_use]
    pub const fn with_unkeyed_ip_user_agent_hash(mut self, hash: UnkeyedIpUserAgentHash) -> Self {
        self.unkeyed_ip_user_agent_hash = hash;
        self
    }
}

/// Compute a best-effort device hash.
///
/// If `key` is present, the output is `key_id:hex(hmac_sha256(...))`.
/// Otherwise the output is a raw SHA-256 hex digest. Device id is preferred
/// over IP/user-agent because it is more stable and less privacy-sensitive.
#[must_use]
pub fn compute_device_hash(
    input: DeviceHashInput<'_>,
    key: Option<&DeviceHashKey>,
) -> Option<String> {
    compute_device_hash_with_policy(input, key, &DeviceHashPolicy::default())
}

/// Compute a best-effort device hash with a product-owned policy.
#[must_use]
pub fn compute_device_hash_with_policy(
    input: DeviceHashInput<'_>,
    key: Option<&DeviceHashKey>,
    policy: &DeviceHashPolicy,
) -> Option<String> {
    let material = device_hash_material(input, policy)?;
    Some(match key {
        Some(key) => {
            let mut mac = HmacSha256::new_from_slice(&key.secret).expect("HMAC accepts any key");
            update_hash_material(&mut mac, &material, &policy.domain_separator);
            format!(
                "{}:{}",
                key.key_id,
                hex::encode(mac.finalize().into_bytes())
            )
        }
        None => {
            let mut hasher = Sha256::new();
            match (&material, policy.unkeyed_device_id_hash) {
                (DeviceHashMaterial::DeviceId(device_id), UnkeyedDeviceIdHash::RawSha256) => {
                    Digest::update(&mut hasher, device_id.as_bytes());
                }
                _ => match (&material, policy.unkeyed_ip_user_agent_hash) {
                    (
                        DeviceHashMaterial::IpUserAgent { ip, user_agent },
                        UnkeyedIpUserAgentHash::LegacyLenPrefixed,
                    ) => {
                        let (Some(ip), Some(user_agent)) = (ip, user_agent) else {
                            return None;
                        };
                        update_len_prefixed(&mut hasher, ip);
                        update_len_prefixed(&mut hasher, user_agent);
                    }
                    _ => update_hash_material(&mut hasher, &material, &policy.domain_separator),
                },
            }
            hex::encode(hasher.finalize())
        }
    })
}

enum DeviceHashMaterial {
    DeviceId(String),
    IpUserAgent {
        ip: Option<String>,
        user_agent: Option<String>,
    },
}

fn device_hash_material(
    input: DeviceHashInput<'_>,
    policy: &DeviceHashPolicy,
) -> Option<DeviceHashMaterial> {
    if let Some(device_id) =
        normalize_device_id_for_hash(input.device_id, policy.device_id_normalization)
    {
        return Some(DeviceHashMaterial::DeviceId(device_id));
    }

    let (ip, user_agent) = match policy.fallback_normalization {
        FallbackMaterialNormalization::TrimmedVisible {
            ip_max_chars,
            user_agent_max_chars,
        } => (
            normalized_visible(input.ip, ip_max_chars).map(ToOwned::to_owned),
            normalized_visible(input.user_agent, user_agent_max_chars).map(ToOwned::to_owned),
        ),
        FallbackMaterialNormalization::RawPresent => (
            input.ip.map(ToOwned::to_owned),
            input.user_agent.map(ToOwned::to_owned),
        ),
    };
    if ip.is_none() && user_agent.is_none() {
        None
    } else {
        Some(DeviceHashMaterial::IpUserAgent { ip, user_agent })
    }
}

fn update_hash_material(
    digest: &mut impl DigestUpdate,
    material: &DeviceHashMaterial,
    domain_separator: &str,
) {
    digest.update_bytes(domain_separator.as_bytes());
    digest.update_bytes(b":");
    match material {
        DeviceHashMaterial::DeviceId(device_id) => {
            digest.update_bytes(b"device-id:");
            update_len_prefixed(digest, device_id);
        }
        DeviceHashMaterial::IpUserAgent { ip, user_agent } => {
            digest.update_bytes(b"ip-ua:");
            if let Some(ip) = ip {
                digest.update_bytes(b"ip:");
                update_len_prefixed(digest, ip);
            }
            if let Some(user_agent) = user_agent {
                digest.update_bytes(b"ua:");
                update_len_prefixed(digest, user_agent);
            }
        }
    }
}

fn normalize_device_id_for_hash(
    value: Option<&str>,
    normalization: DeviceIdNormalization,
) -> Option<String> {
    match normalization {
        DeviceIdNormalization::StrictVisible => value
            .and_then(|value| DeviceId::new(value).ok())
            .map(|device_id| device_id.0),
        DeviceIdNormalization::Trimmed {
            max_bytes,
            reject_control,
        } => value.and_then(|value| {
            let value = value.trim();
            if value.is_empty()
                || value.len() > max_bytes
                || (reject_control && value.chars().any(char::is_control))
            {
                None
            } else {
                Some(value.to_owned())
            }
        }),
    }
}

trait DigestUpdate {
    fn update_bytes(&mut self, bytes: &[u8]);
}

impl DigestUpdate for Sha256 {
    fn update_bytes(&mut self, bytes: &[u8]) {
        Digest::update(self, bytes);
    }
}

impl DigestUpdate for HmacSha256 {
    fn update_bytes(&mut self, bytes: &[u8]) {
        Mac::update(self, bytes);
    }
}

fn update_len_prefixed(digest: &mut impl DigestUpdate, value: &str) {
    digest.update_bytes(&(value.len() as u32).to_be_bytes());
    digest.update_bytes(value.as_bytes());
}

fn validate_visible_trimmed<'a>(
    value: &'a str,
    field: &'static str,
    max_chars: usize,
) -> Result<&'a str, AuthDeviceError> {
    if value.trim() != value {
        return Err(AuthDeviceError::Invalid { field });
    }
    if value.is_empty() {
        return Err(AuthDeviceError::Empty { field });
    }
    if value.chars().count() > max_chars || value.chars().any(char::is_control) {
        return Err(AuthDeviceError::Invalid { field });
    }
    Ok(value)
}

fn normalized_visible(value: Option<&str>, max_chars: usize) -> Option<&str> {
    value.and_then(|value| {
        let value = value.trim();
        if value.is_empty()
            || value.chars().count() > max_chars
            || value.chars().any(char::is_control)
        {
            None
        } else {
            Some(value)
        }
    })
}

fn validate_key_id(value: &str) -> Result<&str, AuthDeviceError> {
    if value.trim() != value {
        return Err(AuthDeviceError::Invalid { field: "key_id" });
    }
    if value.is_empty() {
        return Err(AuthDeviceError::Empty { field: "key_id" });
    }
    if value.len() > 32
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(AuthDeviceError::Invalid { field: "key_id" });
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_label_falls_back_for_unsafe_input() {
        assert_eq!(
            SessionLabel::optional_or_default(Some("Chrome / macOS"), "Web browser")
                .unwrap()
                .as_str(),
            "Chrome / macOS"
        );
        assert_eq!(
            SessionLabel::optional_or_default(Some(" bad "), "Web browser")
                .unwrap()
                .as_str(),
            "Web browser"
        );
        assert!(SessionLabel::optional_or_default(Some(" bad "), " fallback ").is_err());
    }

    #[test]
    fn device_hash_prefers_device_id_and_can_be_hmac_keyed() {
        let input = DeviceHashInput::new()
            .with_device_id(Some("device-1"))
            .with_ip(Some("203.0.113.10"))
            .with_user_agent(Some("Browser"));
        let plain = compute_device_hash(input, None).unwrap();
        let same_without_ip = compute_device_hash(
            DeviceHashInput::new().with_device_id(Some("device-1")),
            None,
        )
        .unwrap();
        assert_eq!(plain, same_without_ip);

        let key = DeviceHashKey::with_key_id("k1", b"secret").unwrap();
        let keyed = compute_device_hash(input, Some(&key)).unwrap();
        assert!(keyed.starts_with("k1:"));
        assert_ne!(plain, keyed);
    }

    #[test]
    fn device_hash_key_debug_redacts_secret_material() {
        let key = DeviceHashKey::with_key_id("k1", b"A").unwrap();
        let debug = format!("{key:?}");
        assert!(debug.contains("DeviceHashKey"));
        assert!(debug.contains("k1"));
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("65"));
    }

    #[test]
    fn device_hash_policy_can_preserve_legacy_device_id_hashes() {
        let policy = DeviceHashPolicy::new()
            .with_domain_separator("legacy-device-hash:v1")
            .unwrap()
            .with_device_id_normalization(DeviceIdNormalization::Trimmed {
                max_bytes: 128,
                reject_control: false,
            })
            .with_unkeyed_device_id_hash(UnkeyedDeviceIdHash::RawSha256);
        let input = DeviceHashInput::new().with_device_id(Some(" device-1 "));

        let unkeyed = compute_device_hash_with_policy(input, None, &policy).unwrap();
        assert_eq!(unkeyed, hex::encode(Sha256::digest(b"device-1")));

        let key = DeviceHashKey::with_key_id_or_default(Some(" bad key "), b"secret").unwrap();
        let keyed = compute_device_hash_with_policy(input, Some(&key), &policy).unwrap();
        let mut expected = HmacSha256::new_from_slice(b"secret").unwrap();
        Mac::update(&mut expected, b"legacy-device-hash:v1:device-id:");
        Mac::update(&mut expected, &(8_u32).to_be_bytes());
        Mac::update(&mut expected, b"device-1");
        assert_eq!(
            keyed,
            format!("v1:{}", hex::encode(expected.finalize().into_bytes()))
        );
    }

    #[test]
    fn device_hash_policy_can_preserve_legacy_ip_user_agent_fallbacks() {
        let policy = DeviceHashPolicy::new()
            .with_fallback_normalization(FallbackMaterialNormalization::RawPresent)
            .with_unkeyed_ip_user_agent_hash(UnkeyedIpUserAgentHash::LegacyLenPrefixed);
        let input = DeviceHashInput::new()
            .with_ip(Some("203.0.113.10"))
            .with_user_agent(Some("Browser"));

        let unkeyed = compute_device_hash_with_policy(input, None, &policy).unwrap();
        let mut expected = Sha256::new();
        Digest::update(&mut expected, (12_u32).to_be_bytes());
        Digest::update(&mut expected, b"203.0.113.10");
        Digest::update(&mut expected, (7_u32).to_be_bytes());
        Digest::update(&mut expected, b"Browser");
        assert_eq!(unkeyed, hex::encode(expected.finalize()));

        assert_eq!(
            compute_device_hash_with_policy(
                DeviceHashInput::new().with_ip(Some("same")),
                None,
                &policy
            ),
            None
        );
        assert_eq!(
            compute_device_hash_with_policy(
                DeviceHashInput::new().with_user_agent(Some("same")),
                None,
                &policy
            ),
            None
        );
    }

    #[test]
    fn device_hash_uses_ip_ua_when_device_id_is_absent() {
        let input = DeviceHashInput::new()
            .with_ip(Some("203.0.113.10"))
            .with_user_agent(Some("Browser"));
        assert!(compute_device_hash(input, None).is_some());
        assert!(compute_device_hash(DeviceHashInput::new(), None).is_none());
    }

    #[test]
    fn validation_rejects_padded_or_control_values() {
        assert!(DeviceId::new(" abc").is_err());
        assert!(DeviceId::new("abc\n").is_err());
        assert!(DeviceHashKey::with_key_id("bad key", b"secret").is_err());
        assert!(DeviceHashKey::new(b"").is_err());
    }
}
