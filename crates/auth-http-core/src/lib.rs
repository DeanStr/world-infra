//! HTTP-edge helpers for product-owned authentication flows.
//!
//! This crate owns small, security-sensitive mechanics that are easy to
//! duplicate incorrectly: refresh cookie string construction, safe cookie
//! clearing, trusted-origin checks for cookie-auth endpoints, and reusable auth
//! failure categories. Products still own route handlers, account/session
//! lookup, CORS policy, CSRF strategy, and API response bodies.

use std::{error::Error, fmt, net::IpAddr, time::Duration};

pub use auth_primitives::{parse_bearer_authorization, BearerCredential};

/// HTTP auth helper error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthHttpError {
    /// Cookie name was blank or unsafe.
    InvalidCookieName,
    /// Cookie value was unsafe.
    InvalidCookieValue,
    /// Cookie domain was unsafe.
    InvalidCookieDomain,
    /// Cookie path was unsafe.
    InvalidCookiePath,
    /// `__Host-` cookies must be secure, path `/`, and domainless.
    InvalidHostPrefixCookie,
    /// Request origin/referrer was not trusted.
    UntrustedOrigin,
}

impl fmt::Display for AuthHttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCookieName => f.write_str("cookie name is invalid"),
            Self::InvalidCookieValue => f.write_str("cookie value is invalid"),
            Self::InvalidCookieDomain => f.write_str("cookie domain is invalid"),
            Self::InvalidCookiePath => f.write_str("cookie path is invalid"),
            Self::InvalidHostPrefixCookie => f.write_str("__Host- cookie attributes are invalid"),
            Self::UntrustedOrigin => f.write_str("request origin is not trusted"),
        }
    }
}

impl Error for AuthHttpError {}

/// Product-neutral auth failure category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AuthFailureKind {
    /// Missing or invalid authentication.
    Unauthorized,
    /// Authenticated subject is not allowed.
    Forbidden,
    /// Request exceeded an auth-related rate limit.
    RateLimited,
    /// Auth dependency was unavailable.
    ServiceUnavailable,
    /// Request shape was invalid.
    BadRequest,
}

/// Refresh-cookie `SameSite` attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SameSite {
    /// `SameSite=Lax`.
    Lax,
    /// `SameSite=Strict`.
    Strict,
    /// `SameSite=None`.
    None,
}

impl SameSite {
    fn as_str(self) -> &'static str {
        match self {
            Self::Lax => "Lax",
            Self::Strict => "Strict",
            Self::None => "None",
        }
    }
}

/// Refresh-cookie construction settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshCookieConfig {
    /// Cookie name.
    pub name: String,
    /// Cookie path.
    pub path: String,
    /// Optional cookie domain.
    pub domain: Option<String>,
    /// Whether to add `Secure`.
    pub secure: bool,
    /// Whether to add `HttpOnly`.
    pub http_only: bool,
    /// SameSite attribute.
    pub same_site: SameSite,
    /// Max age for a set cookie.
    pub max_age: Duration,
}

impl RefreshCookieConfig {
    /// Construct secure `__Host-` refresh-cookie settings.
    ///
    /// # Errors
    ///
    /// Returns [`AuthHttpError`] for invalid cookie names or names without the
    /// `__Host-` prefix.
    pub fn host_prefix(name: impl Into<String>, max_age: Duration) -> Result<Self, AuthHttpError> {
        let name = name.into();
        if !name.starts_with("__Host-") {
            return Err(AuthHttpError::InvalidHostPrefixCookie);
        }
        let config = Self {
            name,
            path: "/".to_owned(),
            domain: None,
            secure: true,
            http_only: true,
            same_site: SameSite::Lax,
            max_age,
        };
        config.validate()?;
        Ok(config)
    }

    /// Validate the config.
    ///
    /// # Errors
    ///
    /// Returns [`AuthHttpError`] for invalid attributes.
    pub fn validate(&self) -> Result<(), AuthHttpError> {
        validate_cookie_name(&self.name)?;
        validate_cookie_path(&self.path)?;
        if let Some(domain) = &self.domain {
            validate_cookie_domain(domain)?;
        }
        if self.name.starts_with("__Host-")
            && (!self.secure || self.path != "/" || self.domain.is_some())
        {
            return Err(AuthHttpError::InvalidHostPrefixCookie);
        }
        if matches!(self.same_site, SameSite::None) && !self.secure {
            return Err(AuthHttpError::InvalidCookieValue);
        }
        Ok(())
    }

    /// Build a `Set-Cookie` header value that stores a refresh token.
    ///
    /// # Errors
    ///
    /// Returns [`AuthHttpError`] for unsafe cookie attributes or values.
    pub fn build_set_cookie(&self, value: &str) -> Result<String, AuthHttpError> {
        self.validate()?;
        validate_cookie_value(value)?;
        let max_age = duration_secs(self.max_age);
        let mut cookie = format!(
            "{}={value}; Path={}; Max-Age={max_age}",
            self.name, self.path
        );
        if let Some(domain) = &self.domain {
            cookie.push_str("; Domain=");
            cookie.push_str(domain);
        }
        if self.http_only {
            cookie.push_str("; HttpOnly");
        }
        if self.secure {
            cookie.push_str("; Secure");
        }
        cookie.push_str("; SameSite=");
        cookie.push_str(self.same_site.as_str());
        Ok(cookie)
    }

    /// Build a `Set-Cookie` header value that clears the refresh cookie.
    ///
    /// # Errors
    ///
    /// Returns [`AuthHttpError`] for unsafe cookie attributes.
    pub fn build_clear_cookie(&self) -> Result<String, AuthHttpError> {
        self.validate()?;
        let mut cookie = format!(
            "{}=; Path={}; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT",
            self.name, self.path
        );
        if let Some(domain) = &self.domain {
            cookie.push_str("; Domain=");
            cookie.push_str(domain);
        }
        if self.http_only {
            cookie.push_str("; HttpOnly");
        }
        if self.secure {
            cookie.push_str("; Secure");
        }
        cookie.push_str("; SameSite=");
        cookie.push_str(self.same_site.as_str());
        Ok(cookie)
    }
}

fn duration_secs(duration: Duration) -> u64 {
    duration
        .as_secs()
        .saturating_add(u64::from(duration.subsec_nanos() != 0))
        .max(1)
}

fn validate_cookie_name(name: &str) -> Result<(), AuthHttpError> {
    if name.is_empty()
        || name.len() > 128
        || name
            .bytes()
            .any(|byte| !matches!(byte, b'!' | b'#'..=b'\'' | b'*' | b'+' | b'-' | b'.' | b'0'..=b'9' | b'A'..=b'Z' | b'^' | b'_' | b'`' | b'a'..=b'z' | b'|' | b'~'))
    {
        return Err(AuthHttpError::InvalidCookieName);
    }
    Ok(())
}

fn validate_cookie_value(value: &str) -> Result<(), AuthHttpError> {
    if value.is_empty()
        || value.bytes().any(|byte| {
            !byte.is_ascii()
                || byte.is_ascii_control()
                || byte.is_ascii_whitespace()
                || matches!(byte, b'"' | b';' | b',' | b'\\')
        })
    {
        return Err(AuthHttpError::InvalidCookieValue);
    }
    Ok(())
}

fn validate_cookie_path(path: &str) -> Result<(), AuthHttpError> {
    if !path.starts_with('/')
        || path
            .bytes()
            .any(|byte| !byte.is_ascii() || byte.is_ascii_control() || byte == b';')
    {
        return Err(AuthHttpError::InvalidCookiePath);
    }
    Ok(())
}

fn validate_cookie_domain(domain: &str) -> Result<(), AuthHttpError> {
    if domain.trim() != domain {
        return Err(AuthHttpError::InvalidCookieDomain);
    }
    if domain.is_empty()
        || domain.starts_with('.')
        || domain.ends_with('.')
        || domain.contains("..")
        || !domain.split('.').all(valid_cookie_domain_label)
    {
        return Err(AuthHttpError::InvalidCookieDomain);
    }
    Ok(())
}

fn valid_cookie_domain_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// Trusted-origin policy for cookie-auth endpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedOriginPolicy {
    allowed_origins: Vec<String>,
    require_origin_or_referer: bool,
}

impl TrustedOriginPolicy {
    /// Construct a policy from exact origin strings.
    ///
    /// # Errors
    ///
    /// Returns [`AuthHttpError::UntrustedOrigin`] when any origin is blank or
    /// malformed.
    pub fn new(
        allowed_origins: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<Self, AuthHttpError> {
        let mut allowed = Vec::new();
        for origin in allowed_origins {
            allowed.push(normalize_origin(origin.as_ref())?);
        }
        Ok(Self {
            allowed_origins: allowed,
            require_origin_or_referer: true,
        })
    }

    /// Return a copy that allows missing `Origin` and `Referer`.
    #[must_use]
    pub fn allow_missing_origin_or_referer(mut self) -> Self {
        self.require_origin_or_referer = false;
        self
    }

    /// Return whether a request's origin/referer is trusted.
    #[must_use]
    pub fn is_trusted(&self, origin: Option<&str>, referer: Option<&str>) -> bool {
        if let Some(origin) = origin {
            return normalize_origin(origin).ok().is_some_and(|origin| {
                self.allowed_origins
                    .iter()
                    .any(|allowed| allowed == &origin)
            });
        }
        if let Some(referer) = referer {
            return origin_from_url(referer).ok().is_some_and(|origin| {
                self.allowed_origins
                    .iter()
                    .any(|allowed| allowed == &origin)
            });
        }
        !self.require_origin_or_referer
    }

    /// Require that a request's origin/referer is trusted.
    ///
    /// # Errors
    ///
    /// Returns [`AuthHttpError::UntrustedOrigin`] if the request is not trusted.
    pub fn require_trusted(
        &self,
        origin: Option<&str>,
        referer: Option<&str>,
    ) -> Result<(), AuthHttpError> {
        if self.is_trusted(origin, referer) {
            Ok(())
        } else {
            Err(AuthHttpError::UntrustedOrigin)
        }
    }
}

fn normalize_origin(origin: &str) -> Result<String, AuthHttpError> {
    let origin = origin.trim();
    if origin.is_empty() || origin.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(AuthHttpError::UntrustedOrigin);
    }
    let Some((scheme, rest)) = origin.split_once("://") else {
        return Err(AuthHttpError::UntrustedOrigin);
    };
    let scheme = scheme.to_ascii_lowercase();
    if !matches!(scheme.as_str(), "http" | "https")
        || rest.is_empty()
        || rest.contains(['/', '?', '#', '\\'])
        || rest.contains('@')
        || rest.bytes().any(|byte| byte.is_ascii_whitespace())
    {
        return Err(AuthHttpError::UntrustedOrigin);
    }
    validate_authority(rest)?;
    Ok(format!("{}://{}", scheme, rest.to_ascii_lowercase()))
}

fn origin_from_url(url: &str) -> Result<String, AuthHttpError> {
    let url = url.trim();
    let Some((scheme, rest)) = url.split_once("://") else {
        return Err(AuthHttpError::UntrustedOrigin);
    };
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .filter(|authority| !authority.is_empty())
        .ok_or(AuthHttpError::UntrustedOrigin)?;
    normalize_origin(&format!("{scheme}://{authority}"))
}

fn validate_authority(authority: &str) -> Result<(), AuthHttpError> {
    let (host, port, bracketed) = split_host_port(authority)?;
    if let Some(port) = port {
        validate_port(port)?;
    }
    if bracketed {
        return match host.parse::<IpAddr>() {
            Ok(IpAddr::V6(_)) => Ok(()),
            _ => Err(AuthHttpError::UntrustedOrigin),
        };
    }
    validate_host(host)
}

fn split_host_port(authority: &str) -> Result<(&str, Option<&str>, bool), AuthHttpError> {
    if let Some(rest) = authority.strip_prefix('[') {
        let Some((host, after)) = rest.split_once(']') else {
            return Err(AuthHttpError::UntrustedOrigin);
        };
        if after.is_empty() {
            return Ok((host, None, true));
        }
        let Some(port) = after.strip_prefix(':') else {
            return Err(AuthHttpError::UntrustedOrigin);
        };
        return Ok((host, Some(port), true));
    }

    if authority.contains(']') || authority.contains('[') {
        return Err(AuthHttpError::UntrustedOrigin);
    }
    if authority.matches(':').count() > 1 {
        return Err(AuthHttpError::UntrustedOrigin);
    }
    Ok(match authority.rsplit_once(':') {
        Some((host, port)) => (host, Some(port), false),
        None => (authority, None, false),
    })
}

fn validate_port(port: &str) -> Result<(), AuthHttpError> {
    if port.is_empty() || port.len() > 5 || !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(AuthHttpError::UntrustedOrigin);
    }
    let parsed = port
        .parse::<u16>()
        .map_err(|_| AuthHttpError::UntrustedOrigin)?;
    if parsed == 0 {
        return Err(AuthHttpError::UntrustedOrigin);
    }
    Ok(())
}

fn validate_host(host: &str) -> Result<(), AuthHttpError> {
    if host.is_empty() {
        return Err(AuthHttpError::UntrustedOrigin);
    }
    if host.parse::<IpAddr>().is_ok() {
        return Ok(());
    }
    if host.contains(':') {
        return Err(AuthHttpError::UntrustedOrigin);
    }
    if host.eq_ignore_ascii_case("localhost") {
        return Ok(());
    }
    if host.starts_with('.') || host.ends_with('.') || host.contains("..") {
        return Err(AuthHttpError::UntrustedOrigin);
    }
    for label in host.split('.') {
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(AuthHttpError::UntrustedOrigin);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_prefix_cookie_builds_set_and_clear_headers() {
        let config =
            RefreshCookieConfig::host_prefix("__Host-chairman_refresh", Duration::from_secs(60))
                .unwrap();
        let set = config.build_set_cookie("refresh-token").unwrap();
        assert!(set.contains("__Host-chairman_refresh=refresh-token"));
        assert!(set.contains("Path=/"));
        assert!(set.contains("Max-Age=60"));
        assert!(set.contains("HttpOnly"));
        assert!(set.contains("Secure"));
        assert!(set.contains("SameSite=Lax"));

        let clear = config.build_clear_cookie().unwrap();
        assert!(clear.contains("Max-Age=0"));
        assert!(clear.contains("Expires=Thu, 01 Jan 1970"));
        assert_eq!(
            RefreshCookieConfig::host_prefix("chairman_refresh", Duration::from_secs(60)),
            Err(AuthHttpError::InvalidHostPrefixCookie)
        );
    }

    #[test]
    fn cookie_validation_rejects_header_injection_and_bad_host_prefix() {
        let config =
            RefreshCookieConfig::host_prefix("__Host-chairman_refresh", Duration::from_secs(60))
                .unwrap();
        assert_eq!(
            config.build_set_cookie("bad;token"),
            Err(AuthHttpError::InvalidCookieValue)
        );
        assert_eq!(
            config.build_set_cookie("bad token"),
            Err(AuthHttpError::InvalidCookieValue)
        );
        assert_eq!(
            config.build_set_cookie("caf\u{e9}"),
            Err(AuthHttpError::InvalidCookieValue)
        );
        let bad = RefreshCookieConfig {
            secure: false,
            ..config
        };
        assert_eq!(bad.validate(), Err(AuthHttpError::InvalidHostPrefixCookie));
        let bad_same_site = RefreshCookieConfig {
            name: "refresh".to_owned(),
            path: "/".to_owned(),
            domain: None,
            secure: false,
            http_only: true,
            same_site: SameSite::None,
            max_age: Duration::from_secs(60),
        };
        assert_eq!(
            bad_same_site.validate(),
            Err(AuthHttpError::InvalidCookieValue)
        );
        let bad_domain = RefreshCookieConfig {
            name: "refresh".to_owned(),
            path: "/".to_owned(),
            domain: Some(" example.com ".to_owned()),
            secure: true,
            http_only: true,
            same_site: SameSite::Lax,
            max_age: Duration::from_secs(60),
        };
        assert_eq!(
            bad_domain.validate(),
            Err(AuthHttpError::InvalidCookieDomain)
        );
        let bad_path = RefreshCookieConfig {
            name: "refresh".to_owned(),
            path: "/caf\u{e9}".to_owned(),
            domain: None,
            secure: true,
            http_only: true,
            same_site: SameSite::Lax,
            max_age: Duration::from_secs(60),
        };
        assert_eq!(bad_path.validate(), Err(AuthHttpError::InvalidCookiePath));
        for domain in ["-example.com", "example-.com", "example.-com"] {
            let bad_domain = RefreshCookieConfig {
                name: "refresh".to_owned(),
                path: "/".to_owned(),
                domain: Some(domain.to_owned()),
                secure: true,
                http_only: true,
                same_site: SameSite::Lax,
                max_age: Duration::from_secs(60),
            };
            assert_eq!(
                bad_domain.validate(),
                Err(AuthHttpError::InvalidCookieDomain)
            );
        }
    }

    #[test]
    fn trusted_origin_policy_accepts_origin_or_referer_exactly() {
        let policy =
            TrustedOriginPolicy::new(["https://app.example.com", "http://localhost:5173"]).unwrap();
        assert!(policy.is_trusted(Some("https://app.example.com"), None));
        assert!(policy.is_trusted(None, Some("https://app.example.com/account")));
        assert!(!policy.is_trusted(Some("https://evil.example.com"), None));
        assert!(!policy.is_trusted(None, None));
        assert!(policy
            .clone()
            .allow_missing_origin_or_referer()
            .is_trusted(None, None));
    }

    #[test]
    fn trusted_origin_policy_rejects_malformed_authorities() {
        assert!(TrustedOriginPolicy::new(["https://exa[mple.com"]).is_err());
        assert!(TrustedOriginPolicy::new(["https://a..b.example"]).is_err());
        assert!(TrustedOriginPolicy::new(["https://app.example.com\\evil"]).is_err());
        assert!(TrustedOriginPolicy::new(["https://app.example.com/"]).is_err());
        assert!(TrustedOriginPolicy::new(["https://user@app.example.com"]).is_err());
        assert!(TrustedOriginPolicy::new(["https://[not-ip]"]).is_err());
        assert!(TrustedOriginPolicy::new(["https://[::1]:443"]).is_ok());
        assert!(TrustedOriginPolicy::new(["HTTPS://APP.EXAMPLE.COM"]).is_ok());

        let policy = TrustedOriginPolicy::new(["https://app.example.com"]).unwrap();
        assert!(!policy.is_trusted(Some("https://app.example.com/"), None));
    }

    #[test]
    fn bearer_parser_is_reexported_for_http_adapters() {
        let bearer = parse_bearer_authorization(Some("Bearer token")).unwrap();
        assert_eq!(bearer.token(), "token");
    }
}
