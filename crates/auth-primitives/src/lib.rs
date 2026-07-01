//! Security-neutral authentication value types and validation helpers.
//!
//! This crate intentionally avoids signup, login, refresh rotation, cookie
//! policy, role semantics, admin authorization, product membership checks, and
//! entitlements.

use std::{error::Error, fmt, num::NonZeroU64, str::FromStr};

/// Error returned by auth primitives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthPrimitiveError {
    /// A string value was blank.
    Empty {
        /// Field name.
        field: &'static str,
    },
    /// A string value contained unsupported characters.
    Invalid {
        /// Field name.
        field: &'static str,
    },
    /// Session version must be positive.
    InvalidSessionVersion,
    /// Token type was unknown.
    UnknownTokenType(String),
}

impl fmt::Display for AuthPrimitiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { field } => write!(f, "{field} is empty"),
            Self::Invalid { field } => write!(f, "{field} is invalid"),
            Self::InvalidSessionVersion => f.write_str("session version must be positive"),
            Self::UnknownTokenType(token_type) => write!(f, "unknown token type {token_type}"),
        }
    }
}

impl Error for AuthPrimitiveError {}

/// Positive account/session version used for invalidating older credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionVersion(NonZeroU64);

impl SessionVersion {
    /// Construct a session version.
    ///
    /// # Errors
    ///
    /// Returns [`AuthPrimitiveError::InvalidSessionVersion`] for zero.
    pub const fn new(value: u64) -> Result<Self, AuthPrimitiveError> {
        match NonZeroU64::new(value) {
            Some(value) => Ok(Self(value)),
            None => Err(AuthPrimitiveError::InvalidSessionVersion),
        }
    }

    /// Raw version number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// JWT or token claim type vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum TokenType {
    /// Short-lived API access token.
    Access,
    /// Long-lived or rotating refresh token.
    Refresh,
    /// Bounded admin/support impersonation access token.
    Impersonation,
    /// One-time exchange code/token.
    Exchange,
}

impl TokenType {
    /// Stable lowercase claim value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Access => "access",
            Self::Refresh => "refresh",
            Self::Impersonation => "impersonation",
            Self::Exchange => "exchange",
        }
    }
}

impl fmt::Display for TokenType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TokenType {
    type Err = AuthPrimitiveError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "access" => Ok(Self::Access),
            "refresh" => Ok(Self::Refresh),
            "impersonation" => Ok(Self::Impersonation),
            "exchange" => Ok(Self::Exchange),
            other => Err(AuthPrimitiveError::UnknownTokenType(other.to_owned())),
        }
    }
}

/// Validated audience claim.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Audience(String);

impl Audience {
    /// Validate an audience label.
    ///
    /// # Errors
    ///
    /// Returns [`AuthPrimitiveError`] for blank or unsafe labels.
    pub fn new(value: impl AsRef<str>) -> Result<Self, AuthPrimitiveError> {
        let value = value.as_ref();
        if value.chars().any(char::is_control) {
            return Err(AuthPrimitiveError::Invalid { field: "audience" });
        }
        let value = value.trim();
        if value.is_empty() {
            return Err(AuthPrimitiveError::Empty { field: "audience" });
        }
        if value.len() > 128
            || value
                .chars()
                .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ':' | '.')))
        {
            return Err(AuthPrimitiveError::Invalid { field: "audience" });
        }
        Ok(Self(value.to_owned()))
    }

    /// Access the audience label.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Shared validation shape for product-owned token claims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthClaimShape {
    /// Token type.
    pub token_type: TokenType,
    /// Audience.
    pub audience: Audience,
    /// Subject account/session version.
    pub session_version: SessionVersion,
    /// Actor version for impersonation flows.
    pub actor_session_version: Option<SessionVersion>,
}

impl AuthClaimShape {
    /// Return whether this claim is internally consistent.
    #[must_use]
    pub const fn is_consistent(&self) -> bool {
        matches!(self.token_type, TokenType::Impersonation) == self.actor_session_version.is_some()
    }
}

/// Redact an access token or secret for logs.
#[must_use]
pub fn redact_secret(value: &str) -> String {
    let value = value.trim();
    if value.chars().count() <= 8 {
        return "[redacted]".to_owned();
    }
    let prefix = value.chars().take(4).collect::<String>();
    format!("{prefix}...[redacted]")
}

/// Return whether a request origin is allowed by exact match.
#[must_use]
pub fn origin_allowed(origin: &str, allowed_origins: &[&str]) -> bool {
    let origin = origin.trim();
    !origin.is_empty()
        && allowed_origins
            .iter()
            .any(|allowed| origin == allowed.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impersonation_claim_requires_actor_session_version() {
        let claim = AuthClaimShape {
            token_type: TokenType::Impersonation,
            audience: Audience::new("chairman-api").unwrap(),
            session_version: SessionVersion::new(1).unwrap(),
            actor_session_version: None,
        };
        assert!(!claim.is_consistent());
    }

    #[test]
    fn non_impersonation_claim_rejects_actor_session_version() {
        let claim = AuthClaimShape {
            token_type: TokenType::Access,
            audience: Audience::new("chairman-api").unwrap(),
            session_version: SessionVersion::new(1).unwrap(),
            actor_session_version: Some(SessionVersion::new(2).unwrap()),
        };
        assert!(!claim.is_consistent());
    }

    #[test]
    fn redaction_keeps_only_short_prefix() {
        assert_eq!(redact_secret("abcdef123456"), "abcd...[redacted]");
        assert_eq!(redact_secret("short"), "[redacted]");
    }

    #[test]
    fn token_types_parse_display_and_reject_unknown_values() {
        assert_eq!(" ACCESS ".parse::<TokenType>(), Ok(TokenType::Access));
        assert_eq!("refresh".parse::<TokenType>(), Ok(TokenType::Refresh));
        assert_eq!(
            "impersonation".parse::<TokenType>(),
            Ok(TokenType::Impersonation)
        );
        assert_eq!("exchange".parse::<TokenType>(), Ok(TokenType::Exchange));
        assert_eq!(TokenType::Refresh.to_string(), "refresh");
        assert_eq!(
            "magic".parse::<TokenType>(),
            Err(AuthPrimitiveError::UnknownTokenType("magic".to_owned()))
        );
    }

    #[test]
    fn audience_and_session_version_validate_edges() {
        let audience = Audience::new(" chairman-api:v1 ").unwrap();
        assert_eq!(audience.as_str(), "chairman-api:v1");
        assert_eq!(
            Audience::new(" "),
            Err(AuthPrimitiveError::Empty { field: "audience" })
        );
        assert_eq!(
            Audience::new("bad audience"),
            Err(AuthPrimitiveError::Invalid { field: "audience" })
        );
        assert_eq!(
            Audience::new("chairman-api\n"),
            Err(AuthPrimitiveError::Invalid { field: "audience" })
        );
        assert_eq!(
            Audience::new("a".repeat(129)),
            Err(AuthPrimitiveError::Invalid { field: "audience" })
        );
        assert_eq!(SessionVersion::new(7).unwrap().get(), 7);
        assert_eq!(
            SessionVersion::new(0),
            Err(AuthPrimitiveError::InvalidSessionVersion)
        );
    }

    #[test]
    fn auth_error_display_and_origin_matching_are_stable() {
        assert_eq!(
            AuthPrimitiveError::Empty { field: "audience" }.to_string(),
            "audience is empty"
        );
        assert_eq!(
            AuthPrimitiveError::Invalid { field: "audience" }.to_string(),
            "audience is invalid"
        );
        assert_eq!(
            AuthPrimitiveError::InvalidSessionVersion.to_string(),
            "session version must be positive"
        );
        assert_eq!(
            AuthPrimitiveError::UnknownTokenType("magic".to_owned()).to_string(),
            "unknown token type magic"
        );
        assert!(origin_allowed(
            " https://app.example ",
            &["https://api.example", " https://app.example "]
        ));
        assert!(!origin_allowed("", &["https://app.example"]));
        assert!(!origin_allowed(
            "https://evil.example",
            &["https://app.example"]
        ));
    }
}
