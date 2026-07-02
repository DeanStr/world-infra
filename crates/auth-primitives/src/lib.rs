//! Security-neutral authentication value types and validation helpers.
//!
//! This crate intentionally avoids signup, login, refresh rotation, cookie
//! policy, role semantics, admin authorization, product membership checks, and
//! entitlements.

use std::{error::Error, fmt, str::FromStr};

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
    /// Session version was negative before conversion.
    InvalidSessionVersion,
    /// Token type was unknown.
    UnknownTokenType(String),
}

impl fmt::Display for AuthPrimitiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { field } => write!(f, "{field} is empty"),
            Self::Invalid { field } => write!(f, "{field} is invalid"),
            Self::InvalidSessionVersion => f.write_str("session version must be non-negative"),
            Self::UnknownTokenType(token_type) => write!(f, "unknown token type {token_type}"),
        }
    }
}

impl Error for AuthPrimitiveError {}

/// Standard JWT issuer claim name.
pub const CLAIM_ISSUER: &str = "iss";
/// Standard JWT audience claim name.
pub const CLAIM_AUDIENCE: &str = "aud";
/// Standard JWT subject claim name.
pub const CLAIM_SUBJECT: &str = "sub";
/// Product-neutral token-type claim name.
pub const CLAIM_TOKEN_TYPE: &str = "token_type";
/// Product-neutral session-version claim name.
pub const CLAIM_SESSION_VERSION: &str = "session_version";
/// Product-neutral impersonation actor-id claim name.
pub const CLAIM_ACTOR_ID: &str = "actor_id";
/// Product-neutral impersonation actor-session-version claim name.
pub const CLAIM_ACTOR_SESSION_VERSION: &str = "actor_session_version";

/// Account/session version used for invalidating older credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionVersion(u64);

impl SessionVersion {
    /// Construct a session version.
    ///
    /// This accepts `0` because mature products often start account session
    /// versions at zero and increment them to revoke older credentials.
    pub const fn new(value: u64) -> Result<Self, AuthPrimitiveError> {
        Ok(Self(value))
    }

    /// Raw version number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl TryFrom<i64> for SessionVersion {
    type Error = AuthPrimitiveError;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        let value = u64::try_from(value).map_err(|_| AuthPrimitiveError::InvalidSessionVersion)?;
        Self::new(value)
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
    /// One-time exchange code for bounded admin/support impersonation.
    ImpersonationExchange,
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
            Self::ImpersonationExchange => "impersonation_exchange",
        }
    }

    /// Return whether this token type needs an actor session version.
    #[must_use]
    pub const fn requires_actor_session_version(self) -> bool {
        matches!(self, Self::Impersonation | Self::ImpersonationExchange)
    }

    /// Return whether this token type is acceptable for bearer API auth.
    #[must_use]
    pub const fn accepted_as_bearer(self) -> bool {
        matches!(self, Self::Access | Self::Impersonation)
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
            "impersonation_exchange" => Ok(Self::ImpersonationExchange),
            other => Err(AuthPrimitiveError::UnknownTokenType(other.to_owned())),
        }
    }
}

/// Validated JWT issuer claim.
///
/// JWT `iss` is a case-sensitive `StringOrURI` value. This helper preserves
/// the caller-provided value exactly and only rejects blank, whitespace-padded,
/// control-character, or oversized strings.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Issuer(String);

impl Issuer {
    /// Validate a JWT issuer value.
    ///
    /// # Errors
    ///
    /// Returns [`AuthPrimitiveError`] for blank, malformed, or oversized
    /// issuer values.
    pub fn new(value: impl AsRef<str>) -> Result<Self, AuthPrimitiveError> {
        let value = validate_jwt_string_or_uri(value.as_ref(), "issuer", 512)?;
        Ok(Self(value.to_owned()))
    }

    /// Access the issuer value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validated internal issuer label.
///
/// Use this when a product wants a filesystem/log/key-safe issuer label. Use
/// [`Issuer`] for JWT `iss` values such as `https://api.example.com`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IssuerLabel(String);

impl IssuerLabel {
    /// Validate an issuer label.
    ///
    /// # Errors
    ///
    /// Returns [`AuthPrimitiveError`] for blank or unsafe labels.
    pub fn new(value: impl AsRef<str>) -> Result<Self, AuthPrimitiveError> {
        let value = validate_label(value.as_ref(), "issuer_label", 256)?;
        Ok(Self(value.to_owned()))
    }

    /// Access the issuer label.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
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
        let value = validate_label(value.as_ref(), "audience", 128)?;
        Ok(Self(value.to_owned()))
    }

    /// Access the audience label.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validated actor id for bounded impersonation flows.
///
/// This preserves the caller-provided value exactly so products can use UUIDs,
/// stable account ids, or issuer-scoped actor ids. Product authorization,
/// support-role checks, audit trails, and impersonation duration policy remain
/// local.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActorId(String);

impl ActorId {
    /// Validate an impersonation actor id.
    ///
    /// # Errors
    ///
    /// Returns [`AuthPrimitiveError`] for blank, malformed, or oversized
    /// actor-id values.
    pub fn new(value: impl AsRef<str>) -> Result<Self, AuthPrimitiveError> {
        let value = validate_actor_id(value.as_ref())?;
        Ok(Self(value.to_owned()))
    }

    /// Access the actor id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate_label<'a>(
    value: &'a str,
    field: &'static str,
    max_len: usize,
) -> Result<&'a str, AuthPrimitiveError> {
    if value.chars().any(char::is_control) {
        return Err(AuthPrimitiveError::Invalid { field });
    }
    let value = value.trim();
    if value.is_empty() {
        return Err(AuthPrimitiveError::Empty { field });
    }
    if value.len() > max_len
        || value
            .chars()
            .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ':' | '.')))
    {
        return Err(AuthPrimitiveError::Invalid { field });
    }
    Ok(value)
}

fn validate_jwt_string_or_uri<'a>(
    value: &'a str,
    field: &'static str,
    max_len: usize,
) -> Result<&'a str, AuthPrimitiveError> {
    if value.is_empty() || value.trim().is_empty() {
        return Err(AuthPrimitiveError::Empty { field });
    }
    if value.trim() != value || value.len() > max_len || value.chars().any(char::is_control) {
        return Err(AuthPrimitiveError::Invalid { field });
    }
    Ok(value)
}

fn validate_actor_id(value: &str) -> Result<&str, AuthPrimitiveError> {
    let value = validate_jwt_string_or_uri(value, "actor_id", 512)?;
    if value.chars().any(char::is_whitespace) {
        return Err(AuthPrimitiveError::Invalid { field: "actor_id" });
    }
    Ok(value)
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
        self.token_type.requires_actor_session_version() == self.actor_session_version.is_some()
    }

    /// Return whether this claim is internally consistent with the supplied
    /// actor id.
    ///
    /// This is the safer predicate for products that support impersonation:
    /// impersonation token types must include both actor id and actor session
    /// version, while non-impersonation token types must include neither.
    #[must_use]
    pub const fn is_consistent_with_actor(&self, actor_id: Option<&ActorId>) -> bool {
        let requires_actor = self.token_type.requires_actor_session_version();
        requires_actor == self.actor_session_version.is_some()
            && requires_actor == actor_id.is_some()
    }

    /// Return whether this claim may be used for bearer API authentication.
    #[must_use]
    pub const fn accepted_as_bearer(&self) -> bool {
        self.token_type.accepted_as_bearer() && self.is_consistent()
    }

    /// Return whether this claim may be used for bearer API authentication
    /// when actor id is available separately.
    #[must_use]
    pub const fn accepted_as_bearer_with_actor(&self, actor_id: Option<&ActorId>) -> bool {
        self.token_type.accepted_as_bearer() && self.is_consistent_with_actor(actor_id)
    }
}

/// Shared validation shape for impersonation-specific token claims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpersonationClaimShape {
    /// Token type.
    pub token_type: TokenType,
    /// Actor id for support/admin identity.
    pub actor_id: Option<ActorId>,
    /// Actor session version for invalidating old impersonation credentials.
    pub actor_session_version: Option<SessionVersion>,
}

impl ImpersonationClaimShape {
    /// Construct an impersonation-claim shape from optional actor fields.
    #[must_use]
    pub fn new(
        token_type: TokenType,
        actor_id: Option<ActorId>,
        actor_session_version: Option<SessionVersion>,
    ) -> Self {
        Self {
            token_type,
            actor_id,
            actor_session_version,
        }
    }

    /// Return whether actor fields are valid for the token type.
    ///
    /// Impersonation and impersonation-exchange tokens must include both actor
    /// id and actor session version. Non-impersonation tokens must include
    /// neither. Products still own role checks and audit policy.
    #[must_use]
    pub const fn is_consistent(&self) -> bool {
        let requires_actor = self.token_type.requires_actor_session_version();
        requires_actor == self.actor_id.is_some()
            && requires_actor == self.actor_session_version.is_some()
    }
}

/// Result of comparing a token session version with the current account version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionFreshness {
    /// Token version equals the current account/session version.
    Current,
    /// Token version is older than the current account/session version.
    Stale,
    /// Token version is newer than the current account/session version.
    FromFuture,
}

/// Compare token and current session versions.
#[must_use]
pub const fn session_freshness(
    token_version: SessionVersion,
    current_version: SessionVersion,
) -> SessionFreshness {
    if token_version.get() == current_version.get() {
        SessionFreshness::Current
    } else if token_version.get() < current_version.get() {
        SessionFreshness::Stale
    } else {
        SessionFreshness::FromFuture
    }
}

/// Return whether a token version is current.
#[must_use]
pub const fn session_version_is_current(
    token_version: SessionVersion,
    current_version: SessionVersion,
) -> bool {
    matches!(
        session_freshness(token_version, current_version),
        SessionFreshness::Current
    )
}

/// Compare an actor token session version with the current actor session
/// version.
#[must_use]
pub const fn actor_session_freshness(
    token_version: SessionVersion,
    current_version: SessionVersion,
) -> SessionFreshness {
    session_freshness(token_version, current_version)
}

/// Return whether an actor token session version is current.
#[must_use]
pub const fn actor_session_version_is_current(
    token_version: SessionVersion,
    current_version: SessionVersion,
) -> bool {
    session_version_is_current(token_version, current_version)
}

/// Parsed bearer authorization credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BearerCredential<'a> {
    token: &'a str,
}

impl<'a> BearerCredential<'a> {
    /// Access the bearer token.
    #[must_use]
    pub const fn token(&self) -> &'a str {
        self.token
    }
}

/// Parse an HTTP `Authorization` header as a bearer credential.
///
/// The returned token borrows from the input header. This helper is deliberately
/// strict about scheme shape and RFC 6750 bearer-token characters, while
/// leaving JWT decoding or opaque-token lookup to product-owned code.
///
/// # Errors
///
/// Returns [`AuthPrimitiveError`] for missing, non-bearer, blank, or unsafe
/// values.
pub fn parse_bearer_authorization(
    authorization: Option<&str>,
) -> Result<BearerCredential<'_>, AuthPrimitiveError> {
    let Some(header) = authorization else {
        return Err(AuthPrimitiveError::Empty {
            field: "authorization",
        });
    };
    if header.chars().any(char::is_control) {
        return Err(AuthPrimitiveError::Invalid {
            field: "authorization",
        });
    }
    let header = header.trim();
    if header.is_empty() {
        return Err(AuthPrimitiveError::Empty {
            field: "authorization",
        });
    }
    let mut parts = header.splitn(2, char::is_whitespace);
    let scheme = parts.next().unwrap_or_default();
    let token = parts.next().unwrap_or_default().trim();
    if !scheme.eq_ignore_ascii_case("bearer") || token.is_empty() {
        return Err(AuthPrimitiveError::Invalid {
            field: "authorization",
        });
    }
    if token
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace())
        || !token.bytes().all(is_bearer_token_byte)
    {
        return Err(AuthPrimitiveError::Invalid {
            field: "authorization",
        });
    }
    Ok(BearerCredential { token })
}

fn is_bearer_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'+' | b'/' | b'=')
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
    fn impersonation_claim_shape_requires_actor_id_and_actor_session_version() {
        let actor_id = ActorId::new("support-admin-1").unwrap();
        let actor_version = SessionVersion::new(2).unwrap();

        let valid = ImpersonationClaimShape::new(
            TokenType::Impersonation,
            Some(actor_id.clone()),
            Some(actor_version),
        );
        assert!(valid.is_consistent());

        let missing_actor_id =
            ImpersonationClaimShape::new(TokenType::Impersonation, None, Some(actor_version));
        assert!(!missing_actor_id.is_consistent());

        let missing_actor_version =
            ImpersonationClaimShape::new(TokenType::Impersonation, Some(actor_id.clone()), None);
        assert!(!missing_actor_version.is_consistent());

        let access_with_actor =
            ImpersonationClaimShape::new(TokenType::Access, Some(actor_id), Some(actor_version));
        assert!(!access_with_actor.is_consistent());
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
        assert!(!claim.accepted_as_bearer());
    }

    #[test]
    fn bearer_claims_must_have_consistent_impersonation_shape() {
        let malformed_impersonation = AuthClaimShape {
            token_type: TokenType::Impersonation,
            audience: Audience::new("chairman-api").unwrap(),
            session_version: SessionVersion::new(1).unwrap(),
            actor_session_version: None,
        };
        assert!(!malformed_impersonation.is_consistent());
        assert!(!malformed_impersonation.accepted_as_bearer());

        let valid_impersonation = AuthClaimShape {
            token_type: TokenType::Impersonation,
            audience: Audience::new("chairman-api").unwrap(),
            session_version: SessionVersion::new(1).unwrap(),
            actor_session_version: Some(SessionVersion::new(2).unwrap()),
        };
        let actor_id = ActorId::new("support-admin-1").unwrap();
        assert!(valid_impersonation.is_consistent());
        assert!(valid_impersonation.accepted_as_bearer());
        assert!(!valid_impersonation.accepted_as_bearer_with_actor(None));
        assert!(valid_impersonation.accepted_as_bearer_with_actor(Some(&actor_id)));

        let access_with_actor = AuthClaimShape {
            token_type: TokenType::Access,
            audience: Audience::new("chairman-api").unwrap(),
            session_version: SessionVersion::new(1).unwrap(),
            actor_session_version: None,
        };
        assert!(!access_with_actor.accepted_as_bearer_with_actor(Some(&actor_id)));
    }

    #[test]
    fn actor_ids_claim_names_and_freshness_helpers_are_stable() {
        let actor_id = ActorId::new("https://support.example.com/actors/1").unwrap();
        assert_eq!(actor_id.as_str(), "https://support.example.com/actors/1");
        assert!(ActorId::new(" actor-1 ").is_err());
        assert!(ActorId::new("support admin").is_err());
        assert_eq!(CLAIM_ISSUER, "iss");
        assert_eq!(CLAIM_AUDIENCE, "aud");
        assert_eq!(CLAIM_SUBJECT, "sub");
        assert_eq!(CLAIM_TOKEN_TYPE, "token_type");
        assert_eq!(CLAIM_SESSION_VERSION, "session_version");
        assert_eq!(CLAIM_ACTOR_ID, "actor_id");
        assert_eq!(CLAIM_ACTOR_SESSION_VERSION, "actor_session_version");
        assert_eq!(
            actor_session_freshness(
                SessionVersion::new(1).unwrap(),
                SessionVersion::new(2).unwrap()
            ),
            SessionFreshness::Stale
        );
        assert!(actor_session_version_is_current(
            SessionVersion::new(3).unwrap(),
            SessionVersion::new(3).unwrap()
        ));
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
        assert_eq!(
            "impersonation_exchange".parse::<TokenType>(),
            Ok(TokenType::ImpersonationExchange)
        );
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
        let issuer = IssuerLabel::new(" chairman-api ").unwrap();
        assert_eq!(issuer.as_str(), "chairman-api");
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
        assert_eq!(SessionVersion::new(0).unwrap().get(), 0);
        assert_eq!(SessionVersion::new(7).unwrap().get(), 7);
        assert_eq!(SessionVersion::try_from(7_i64).unwrap().get(), 7);
        assert_eq!(
            SessionVersion::try_from(-1_i64),
            Err(AuthPrimitiveError::InvalidSessionVersion)
        );
    }

    #[test]
    fn issuer_accepts_jwt_string_or_uri_without_normalization() {
        let uri = Issuer::new("https://api.airlinevibe.com").unwrap();
        assert_eq!(uri.as_str(), "https://api.airlinevibe.com");
        let urn = Issuer::new("urn:airline:v1").unwrap();
        assert_eq!(urn.as_str(), "urn:airline:v1");
        let label = Issuer::new("chairman-api").unwrap();
        assert_eq!(label.as_str(), "chairman-api");
        assert_eq!(
            Issuer::new(" https://api.airlinevibe.com "),
            Err(AuthPrimitiveError::Invalid { field: "issuer" })
        );
        assert_eq!(
            Issuer::new("https://api.airlinevibe.com\n"),
            Err(AuthPrimitiveError::Invalid { field: "issuer" })
        );
        assert_eq!(
            Issuer::new(" "),
            Err(AuthPrimitiveError::Empty { field: "issuer" })
        );
        assert_eq!(
            Issuer::new("a".repeat(513)),
            Err(AuthPrimitiveError::Invalid { field: "issuer" })
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
            "session version must be non-negative"
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

    #[test]
    fn bearer_authorization_parser_is_strict_but_case_insensitive() {
        let parsed = parse_bearer_authorization(Some(" bearer   abc.def ")).unwrap();
        assert_eq!(parsed.token(), "abc.def");
        assert_eq!(
            parse_bearer_authorization(None),
            Err(AuthPrimitiveError::Empty {
                field: "authorization"
            })
        );
        assert_eq!(
            parse_bearer_authorization(Some("Basic abc")),
            Err(AuthPrimitiveError::Invalid {
                field: "authorization"
            })
        );
        assert_eq!(
            parse_bearer_authorization(Some("Bearer ")),
            Err(AuthPrimitiveError::Invalid {
                field: "authorization"
            })
        );
        assert_eq!(
            parse_bearer_authorization(Some("Bearer abc\n")),
            Err(AuthPrimitiveError::Invalid {
                field: "authorization"
            })
        );
        assert_eq!(
            parse_bearer_authorization(Some("Bearer abc def")),
            Err(AuthPrimitiveError::Invalid {
                field: "authorization"
            })
        );
        assert_eq!(
            parse_bearer_authorization(Some("Bearer bad;token")),
            Err(AuthPrimitiveError::Invalid {
                field: "authorization"
            })
        );
        assert_eq!(
            parse_bearer_authorization(Some("Bearer café")),
            Err(AuthPrimitiveError::Invalid {
                field: "authorization"
            })
        );
    }

    #[test]
    fn claim_shape_and_session_freshness_model_airline_auth_posture() {
        let access = AuthClaimShape {
            token_type: TokenType::Access,
            audience: Audience::new("airline-api").unwrap(),
            session_version: SessionVersion::new(2).unwrap(),
            actor_session_version: None,
        };
        assert!(access.is_consistent());
        assert!(access.accepted_as_bearer());

        let refresh = AuthClaimShape {
            token_type: TokenType::Refresh,
            audience: Audience::new("airline-api").unwrap(),
            session_version: SessionVersion::new(2).unwrap(),
            actor_session_version: None,
        };
        assert!(refresh.is_consistent());
        assert!(!refresh.accepted_as_bearer());

        assert_eq!(
            session_freshness(
                SessionVersion::new(1).unwrap(),
                SessionVersion::new(2).unwrap()
            ),
            SessionFreshness::Stale
        );
        assert!(session_version_is_current(
            SessionVersion::new(2).unwrap(),
            SessionVersion::new(2).unwrap()
        ));
        assert_eq!(
            session_freshness(
                SessionVersion::new(3).unwrap(),
                SessionVersion::new(2).unwrap()
            ),
            SessionFreshness::FromFuture
        );
    }
}
