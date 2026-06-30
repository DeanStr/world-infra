//! Static web runtime-config and security-header validation helpers.
//!
//! Product repositories own exact config keys, deployment targets, CSP
//! exceptions, and framework build steps. This crate provides small validators
//! that make production static artifacts harder to misconfigure.

use std::{error::Error, fmt};

/// Deployment environment class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvKind {
    /// Local development or isolated tests.
    Development,
    /// Shared staging, preview, or release candidate environment.
    Staging,
    /// Public production environment.
    Production,
}

impl EnvKind {
    /// Return whether production-like fail-closed rules should apply.
    #[must_use]
    pub const fn is_production_like(self) -> bool {
        matches!(self, Self::Staging | Self::Production)
    }
}

/// Public URL transport family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PublicUrlKind {
    /// HTTP(S) URL.
    Http,
    /// WebSocket URL.
    WebSocket,
}

/// Error returned by static web hardening helpers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticWebError {
    /// A named value was blank.
    Empty {
        /// Field name.
        name: String,
    },
    /// A URL was malformed.
    InvalidUrl {
        /// Field name.
        name: String,
    },
    /// A production-like URL used an insecure scheme.
    InsecureScheme {
        /// Field name.
        name: String,
        /// Observed scheme.
        scheme: String,
    },
    /// A public config key or value looked secret-like.
    SecretLikeValue {
        /// Config key.
        key: String,
        /// Matching reason.
        reason: &'static str,
    },
    /// A required header was missing.
    MissingHeader {
        /// Header name.
        name: String,
    },
    /// A required header directive was missing.
    MissingHeaderDirective {
        /// Header name.
        name: String,
        /// Required directive.
        directive: String,
    },
}

impl fmt::Display for StaticWebError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { name } => write!(f, "{name} is empty"),
            Self::InvalidUrl { name } => write!(f, "{name} is not a valid public URL"),
            Self::InsecureScheme { name, scheme } => {
                write!(f, "{name} uses insecure scheme {scheme}")
            }
            Self::SecretLikeValue { key, reason } => {
                write!(f, "public config key {key} looks secret-like: {reason}")
            }
            Self::MissingHeader { name } => write!(f, "required security header {name} is missing"),
            Self::MissingHeaderDirective { name, directive } => {
                write!(f, "security header {name} is missing directive {directive}")
            }
        }
    }
}

impl Error for StaticWebError {}

/// Parsed public URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicUrl {
    /// Lowercase URL scheme.
    pub scheme: String,
    /// Host and optional path/query/fragment.
    pub remainder: String,
}

/// Validate a public URL for static web runtime config.
///
/// # Errors
///
/// Returns [`StaticWebError`] for blank, malformed, or insecure production-like
/// URLs.
pub fn validate_public_url(
    name: impl AsRef<str>,
    value: impl AsRef<str>,
    kind: PublicUrlKind,
    env: EnvKind,
) -> Result<PublicUrl, StaticWebError> {
    let name = name.as_ref();
    let value = value.as_ref().trim();
    if value.is_empty() {
        return Err(StaticWebError::Empty {
            name: name.to_owned(),
        });
    }
    let (scheme, remainder) =
        value
            .split_once("://")
            .ok_or_else(|| StaticWebError::InvalidUrl {
                name: name.to_owned(),
            })?;
    let scheme = scheme.to_ascii_lowercase();
    let allowed = match kind {
        PublicUrlKind::Http => matches!(scheme.as_str(), "http" | "https"),
        PublicUrlKind::WebSocket => matches!(scheme.as_str(), "ws" | "wss"),
    };
    if !allowed
        || remainder.is_empty()
        || remainder.starts_with('/')
        || remainder.contains('@')
        || remainder
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
        || host_port(remainder).is_none()
    {
        return Err(StaticWebError::InvalidUrl {
            name: name.to_owned(),
        });
    }
    if env.is_production_like()
        && matches!(
            (kind, scheme.as_str()),
            (PublicUrlKind::Http, "http") | (PublicUrlKind::WebSocket, "ws")
        )
    {
        return Err(StaticWebError::InsecureScheme {
            name: name.to_owned(),
            scheme,
        });
    }
    Ok(PublicUrl {
        scheme,
        remainder: remainder.to_owned(),
    })
}

fn host_port(remainder: &str) -> Option<&str> {
    let authority = remainder
        .split(['/', '?', '#'])
        .next()
        .filter(|authority| !authority.is_empty())?;
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, port) = rest.split_once(']')?;
        if host.is_empty() || port.strip_prefix(':').is_some_and(|port| port.is_empty()) {
            return None;
        }
        return if port.is_empty()
            || port
                .strip_prefix(':')
                .is_some_and(|port| port.chars().all(|ch| ch.is_ascii_digit()))
        {
            Some(authority)
        } else {
            None
        };
    }
    let mut parts = authority.split(':');
    let host = parts.next().filter(|host| !host.is_empty())?;
    let port = parts.next();
    if parts.next().is_some() {
        return None;
    }
    if host.starts_with('.') || host.ends_with('.') {
        return None;
    }
    if let Some(port) = port {
        if port.is_empty() || !port.chars().all(|ch| ch.is_ascii_digit()) {
            return None;
        }
    }
    Some(authority)
}

/// Return whether a public config key/value pair looks secret-like.
#[must_use]
pub fn secret_like_reason(key: &str, value: &str) -> Option<&'static str> {
    let key = key.to_ascii_lowercase();
    if [
        "secret",
        "password",
        "private_key",
        "bearer",
        "session",
        "refresh",
    ]
    .iter()
    .any(|needle| key.contains(needle))
    {
        return Some("secret-like key name");
    }
    if key.contains("token") && !key.contains("turnstile") && !key.contains("csrf") {
        return Some("token-like key name");
    }
    let trimmed = value.trim();
    if trimmed.starts_with("sk_live_")
        || trimmed.starts_with("sk_test_")
        || trimmed.starts_with("-----BEGIN ")
        || trimmed.starts_with("Bearer ")
    {
        return Some("secret-like value prefix");
    }
    None
}

/// Validate that public runtime config contains no obvious secrets.
///
/// # Errors
///
/// Returns [`StaticWebError::SecretLikeValue`] for the first suspicious entry.
pub fn validate_public_config_safe<'a>(
    entries: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<(), StaticWebError> {
    for (key, value) in entries {
        if let Some(reason) = secret_like_reason(key, value) {
            return Err(StaticWebError::SecretLikeValue {
                key: key.to_owned(),
                reason,
            });
        }
    }
    Ok(())
}

/// Assert a named header exists and contains every required directive.
///
/// Header name matching is ASCII case-insensitive. Directive checks are simple
/// substring checks so products can keep their exact CSP/header policy local.
///
/// # Errors
///
/// Returns [`StaticWebError`] for missing headers or directives.
pub fn assert_header_directives<'a>(
    headers: impl IntoIterator<Item = (&'a str, &'a str)>,
    name: &str,
    required_directives: &[&str],
) -> Result<(), StaticWebError> {
    let value = headers
        .into_iter()
        .find(|(header, _)| header.eq_ignore_ascii_case(name))
        .map(|(_, value)| value)
        .ok_or_else(|| StaticWebError::MissingHeader {
            name: name.to_owned(),
        })?;
    for directive in required_directives {
        if directive.trim().is_empty() {
            return Err(StaticWebError::MissingHeaderDirective {
                name: name.to_owned(),
                directive: (*directive).to_owned(),
            });
        }
        if !value.contains(directive) {
            return Err(StaticWebError::MissingHeaderDirective {
                name: name.to_owned(),
                directive: (*directive).to_owned(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_like_urls_must_be_secure() {
        assert!(validate_public_url(
            "PUBLIC_WEB_BASE",
            "https://example.com",
            PublicUrlKind::Http,
            EnvKind::Production
        )
        .is_ok());
        assert!(validate_public_url(
            "PUBLIC_WEB_BASE",
            "http://example.com",
            PublicUrlKind::Http,
            EnvKind::Production
        )
        .is_err());
        assert!(validate_public_url(
            "PUBLIC_WEB_BASE",
            "https://",
            PublicUrlKind::Http,
            EnvKind::Production
        )
        .is_err());
        assert!(validate_public_url(
            "PUBLIC_WEB_BASE",
            "https://example.com:abc",
            PublicUrlKind::Http,
            EnvKind::Production
        )
        .is_err());
        assert!(validate_public_url(
            "PUBLIC_WEB_BASE",
            "https://example.com/path with spaces",
            PublicUrlKind::Http,
            EnvKind::Production
        )
        .is_err());
    }

    #[test]
    fn public_config_rejects_secret_like_entries() {
        assert!(validate_public_config_safe([("PUBLIC_WEB_BASE", "https://example.com")]).is_ok());
        assert!(validate_public_config_safe([("STRIPE_SECRET_KEY", "sk_live_x")]).is_err());
    }
}
