//! Static web runtime-config and security-header validation helpers.
//!
//! Product repositories own exact config keys, deployment targets, CSP
//! exceptions, and framework build steps. This crate provides small validators
//! that make production static artifacts harder to misconfigure.

use std::{
    error::Error,
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};

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

/// Error returned by public URL validation with host-policy checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticWebUrlPolicyError {
    /// Base URL validation failed.
    Url(StaticWebError),
    /// A host was rejected by a production/static hosting policy.
    RejectedHost {
        /// Field name.
        name: String,
        /// Observed host.
        host: String,
        /// Rejection reason.
        reason: &'static str,
    },
}

impl fmt::Display for StaticWebUrlPolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Url(error) => error.fmt(f),
            Self::RejectedHost { name, host, reason } => {
                write!(f, "{name} host {host} is rejected: {reason}")
            }
        }
    }
}

impl Error for StaticWebUrlPolicyError {}

impl From<StaticWebError> for StaticWebUrlPolicyError {
    fn from(error: StaticWebError) -> Self {
        Self::Url(error)
    }
}

/// Parsed public URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicUrl {
    /// Lowercase URL scheme.
    pub scheme: String,
    /// Host and optional path/query/fragment.
    pub remainder: String,
}

/// Optional host checks for production/static runtime URLs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostPolicy {
    /// Reject localhost and `.localhost` names.
    pub reject_localhost: bool,
    /// Reject loopback, private, link-local, and unspecified IP literals.
    pub reject_private_ips: bool,
    /// Reject placeholder/documentation hostnames and documentation IP ranges.
    pub reject_placeholder_hosts: bool,
}

impl HostPolicy {
    /// No host restrictions beyond URL syntax.
    #[must_use]
    pub const fn permissive() -> Self {
        Self {
            reject_localhost: false,
            reject_private_ips: false,
            reject_placeholder_hosts: false,
        }
    }

    /// Production-oriented host restrictions.
    #[must_use]
    pub const fn production() -> Self {
        Self {
            reject_localhost: true,
            reject_private_ips: true,
            reject_placeholder_hosts: true,
        }
    }
}

/// Parsed static header line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticHeader {
    /// Header name.
    pub name: String,
    /// Header value.
    pub value: String,
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
    validate_public_url_base(name.as_ref(), value.as_ref(), kind, env)
}

/// Validate a public URL with optional host policy checks.
///
/// # Errors
///
/// Returns [`StaticWebError`] for blank, malformed, insecure production-like,
/// or policy-rejected URLs.
pub fn validate_public_url_with_host_policy(
    name: impl AsRef<str>,
    value: impl AsRef<str>,
    kind: PublicUrlKind,
    env: EnvKind,
    host_policy: HostPolicy,
) -> Result<PublicUrl, StaticWebUrlPolicyError> {
    let name = name.as_ref();
    let public_url = validate_public_url_base(name, value.as_ref(), kind, env)?;
    let host = authority_host(&public_url.remainder).ok_or_else(|| {
        StaticWebUrlPolicyError::Url(StaticWebError::InvalidUrl {
            name: name.to_owned(),
        })
    })?;
    validate_host_policy(name, host, host_policy)?;
    Ok(public_url)
}

fn validate_public_url_base(
    name: &str,
    value: &str,
    kind: PublicUrlKind,
    env: EnvKind,
) -> Result<PublicUrl, StaticWebError> {
    let value = value.trim();
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
        || remainder.contains('\\')
        || remainder
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
        || authority_host(remainder).is_none()
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

fn authority_host(remainder: &str) -> Option<&str> {
    let authority = remainder
        .split(['/', '?', '#'])
        .next()
        .filter(|authority| !authority.is_empty())?;
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, port) = rest.split_once(']')?;
        if host.is_empty() || port.strip_prefix(':').is_some_and(|port| port.is_empty()) {
            return None;
        }
        if host.parse::<Ipv6Addr>().is_err() {
            return None;
        }
        return if port.is_empty() || port.strip_prefix(':').is_some_and(valid_port) {
            Some(host)
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
    if !valid_unbracketed_host(host) {
        return None;
    }
    if let Some(port) = port {
        if !valid_port(port) {
            return None;
        }
    }
    Some(host)
}

fn valid_unbracketed_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && !host.starts_with('.')
        && !host.ends_with('.')
        && !host.contains("..")
        && host.split('.').all(valid_host_label)
}

fn valid_host_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn valid_port(port: &str) -> bool {
    !port.is_empty() && port.chars().all(|ch| ch.is_ascii_digit()) && port.parse::<u16>().is_ok()
}

fn validate_host_policy(
    name: &str,
    host: &str,
    policy: HostPolicy,
) -> Result<(), StaticWebUrlPolicyError> {
    let normalized = host.trim_matches(['[', ']']).to_ascii_lowercase();
    let parsed_ip = normalized.parse::<IpAddr>().ok();
    let noncanonical_ipv4 = parsed_ip
        .is_none()
        .then(|| parse_noncanonical_ipv4_literal(&normalized))
        .flatten();
    if (policy.reject_private_ips || policy.reject_placeholder_hosts) && noncanonical_ipv4.is_some()
    {
        return Err(StaticWebUrlPolicyError::RejectedHost {
            name: name.to_owned(),
            host: host.to_owned(),
            reason: "non-canonical IPv4 host",
        });
    }
    if policy.reject_localhost && (normalized == "localhost" || normalized.ends_with(".localhost"))
    {
        return Err(StaticWebUrlPolicyError::RejectedHost {
            name: name.to_owned(),
            host: host.to_owned(),
            reason: "localhost host",
        });
    }
    if policy.reject_placeholder_hosts && is_placeholder_host(&normalized) {
        return Err(StaticWebUrlPolicyError::RejectedHost {
            name: name.to_owned(),
            host: host.to_owned(),
            reason: "placeholder host",
        });
    }
    if policy.reject_placeholder_hosts {
        if let Some(ip) = parsed_ip {
            if is_documentation_ip(ip) {
                return Err(StaticWebUrlPolicyError::RejectedHost {
                    name: name.to_owned(),
                    host: host.to_owned(),
                    reason: "documentation IP",
                });
            }
        }
    }
    if policy.reject_private_ips {
        if let Some(ip) = parsed_ip {
            if is_private_like_ip(ip) {
                return Err(StaticWebUrlPolicyError::RejectedHost {
                    name: name.to_owned(),
                    host: host.to_owned(),
                    reason: "private or local IP",
                });
            }
        }
    }
    Ok(())
}

fn is_placeholder_host(host: &str) -> bool {
    matches!(
        host,
        "example"
            | "example.com"
            | "example.org"
            | "example.net"
            | "placeholder"
            | "changeme"
            | "todo"
    ) || host.ends_with(".example")
        || host.ends_with(".example.com")
        || host.ends_with(".example.org")
        || host.ends_with(".example.net")
        || host.ends_with(".invalid")
}

fn parse_noncanonical_ipv4_literal(host: &str) -> Option<Ipv4Addr> {
    let parts = host.split('.').collect::<Vec<_>>();
    if parts.is_empty() || parts.len() > 4 || parts.iter().any(|part| part.is_empty()) {
        return None;
    }

    let mut numbers = Vec::with_capacity(parts.len());
    for part in parts {
        numbers.push(parse_ipv4_number(part)?);
    }

    let last_index = numbers.len() - 1;
    if numbers[..last_index].iter().any(|number| *number > 255) {
        return None;
    }
    let last_limit = 256_u64.pow((5 - numbers.len()) as u32);
    if numbers[last_index] >= last_limit {
        return None;
    }

    let mut value = numbers[last_index];
    for (index, number) in numbers[..last_index].iter().enumerate() {
        value += number << (8 * (3 - index));
    }
    Some(Ipv4Addr::from(value as u32))
}

fn parse_ipv4_number(part: &str) -> Option<u64> {
    if part.is_empty() {
        return None;
    }
    let (digits, radix) = if let Some(rest) = part.strip_prefix("0x") {
        (rest, 16)
    } else if part.len() > 1 && part.starts_with('0') {
        (&part[1..], 8)
    } else {
        (part, 10)
    };
    if digits.is_empty() {
        return Some(0);
    }
    let valid_digits = match radix {
        8 => digits.chars().all(|ch| matches!(ch, '0'..='7')),
        10 => digits.chars().all(|ch| ch.is_ascii_digit()),
        16 => digits.chars().all(|ch| ch.is_ascii_hexdigit()),
        _ => false,
    };
    if !valid_digits {
        return None;
    }
    u64::from_str_radix(digits, radix).ok()
}

fn is_documentation_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            (a == 192 && b == 0 && c == 2)
                || (a == 198 && b == 51 && c == 100)
                || (a == 203 && b == 0 && c == 113)
        }
        IpAddr::V6(ip) => {
            if let Some(mapped) = ipv4_mapped_ipv6(ip) {
                return is_documentation_ip(IpAddr::V4(mapped));
            }
            let segments = ip.segments();
            segments[0] == 0x2001 && segments[1] == 0x0db8
        }
    }
}

fn is_private_like_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || (octets[0] == 100 && (octets[1] & 0b1100_0000) == 0b0100_0000)
        }
        IpAddr::V6(ip) => {
            if let Some(mapped) = ipv4_mapped_ipv6(ip) {
                return is_private_like_ip(IpAddr::V4(mapped));
            }
            ip.is_loopback()
                || ip.is_unspecified()
                || (ip.segments()[0] & 0xfe00) == 0xfc00
                || (ip.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

fn ipv4_mapped_ipv6(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let segments = ip.segments();
    if segments[..5] == [0, 0, 0, 0, 0] && segments[5] == 0xffff {
        Some(Ipv4Addr::new(
            (segments[6] >> 8) as u8,
            segments[6] as u8,
            (segments[7] >> 8) as u8,
            segments[7] as u8,
        ))
    } else {
        None
    }
}

/// Parse simple static hosting header files with `Name: value` lines.
///
/// Blank lines and lines beginning with `#` are ignored. Malformed lines are
/// skipped so products can decide whether to fail separately.
#[must_use]
pub fn parse_static_headers(input: &str) -> Vec<StaticHeader> {
    input
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (name, value) = line.split_once(':')?;
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            Some(StaticHeader {
                name: name.to_owned(),
                value: value.trim().to_owned(),
            })
        })
        .collect()
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
/// Header name matching is ASCII case-insensitive. Directive checks match
/// semicolon/comma-separated directive names or exact directive clauses so
/// products can keep their exact CSP/header policy local without accepting
/// substring false positives.
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
        if !header_has_directive(value, directive) {
            return Err(StaticWebError::MissingHeaderDirective {
                name: name.to_owned(),
                directive: (*directive).to_owned(),
            });
        }
    }
    Ok(())
}

fn header_has_directive(value: &str, directive: &str) -> bool {
    let directive = directive.trim();
    value.split([';', ',']).any(|part| {
        let part = part.trim();
        if part == directive {
            return true;
        }
        let Some(token) = part.split_whitespace().next() else {
            return false;
        };
        let name = token.split_once('=').map_or(token, |(name, _)| name);
        name.eq_ignore_ascii_case(directive)
    })
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
            "https://example.com:65536",
            PublicUrlKind::Http,
            EnvKind::Production
        )
        .is_err());
        assert!(validate_public_url(
            "PUBLIC_WEB_BASE",
            "https://example.com:65535",
            PublicUrlKind::Http,
            EnvKind::Production
        )
        .is_ok());
        assert!(validate_public_url(
            "PUBLIC_WEB_BASE",
            "https://example.com/path with spaces",
            PublicUrlKind::Http,
            EnvKind::Production
        )
        .is_err());
        assert!(validate_public_url(
            "PUBLIC_WEB_BASE",
            "https://[not-ip]",
            PublicUrlKind::Http,
            EnvKind::Production
        )
        .is_err());
        assert!(validate_public_url(
            "PUBLIC_WEB_BASE",
            "https://[8.8.8.8]",
            PublicUrlKind::Http,
            EnvKind::Production
        )
        .is_err());
        assert!(validate_public_url(
            "PUBLIC_WEB_BASE",
            "https://[2001:db8::1]:65536",
            PublicUrlKind::Http,
            EnvKind::Production
        )
        .is_err());
        assert!(validate_public_url(
            "PUBLIC_WEB_BASE",
            "https://[2001:db8::1]:65535",
            PublicUrlKind::Http,
            EnvKind::Production
        )
        .is_ok());
    }

    #[test]
    fn public_config_rejects_secret_like_entries() {
        assert!(validate_public_config_safe([("PUBLIC_WEB_BASE", "https://example.com")]).is_ok());
        assert!(validate_public_config_safe([("STRIPE_SECRET_KEY", "sk_live_x")]).is_err());
        assert_eq!(
            secret_like_reason("PUBLIC_TOKEN", "abc"),
            Some("token-like key name")
        );
        assert_eq!(
            secret_like_reason("TURNSTILE_TOKEN", "public-site-key"),
            None
        );
        assert_eq!(
            secret_like_reason("PUBLIC_KEY", "Bearer abc"),
            Some("secret-like value prefix")
        );
    }

    #[test]
    fn production_host_policy_rejects_local_and_placeholder_hosts() {
        for value in [
            "https://localhost",
            "https://localhost.",
            "https://LOCALHOST",
            "https://127.0.0.1",
            "https://10.0.0.1",
            "https://[::1]",
            "https://[::ffff:127.0.0.1]",
            "https://[::ffff:10.0.0.1]",
            "https://[::ffff:7f00:1]",
            "https://127.1",
            "https://10.1",
            "https://2130706433",
            "https://0x7f000001",
            "https://0177.1",
            "https://example.com",
            "https://Example.COM",
            "https://app.invalid",
            "https://App.Invalid",
            "https://192.0.2.1",
            "https://198.51.100.1",
            "https://203.0.113.1",
            "https://[2001:db8::1]",
            "https://[::ffff:192.0.2.1]",
        ] {
            assert!(
                validate_public_url_with_host_policy(
                    "PUBLIC_WEB_BASE",
                    value,
                    PublicUrlKind::Http,
                    EnvKind::Production,
                    HostPolicy::production(),
                )
                .is_err(),
                "{value} should be rejected"
            );
        }
        assert!(validate_public_url_with_host_policy(
            "PUBLIC_WEB_BASE",
            "https://airlinevibe.com",
            PublicUrlKind::Http,
            EnvKind::Production,
            HostPolicy::production(),
        )
        .is_ok());
        assert!(validate_public_url_with_host_policy(
            "PUBLIC_WEB_BASE",
            "https://123.airlinevibe.com",
            PublicUrlKind::Http,
            EnvKind::Production,
            HostPolicy::production(),
        )
        .is_ok());
    }

    #[test]
    fn public_urls_reject_backslash_authority_ambiguity() {
        for value in [
            "https://127.0.0.1\\app",
            "https://localhost\\app",
            "https://airlinevibe.com\\app",
        ] {
            assert!(
                matches!(
                    validate_public_url_with_host_policy(
                        "PUBLIC_WEB_BASE",
                        value,
                        PublicUrlKind::Http,
                        EnvKind::Production,
                        HostPolicy::production(),
                    ),
                    Err(StaticWebUrlPolicyError::Url(
                        StaticWebError::InvalidUrl { .. }
                    ))
                ),
                "{value} should be rejected as an invalid URL"
            );
        }
    }

    #[test]
    fn public_urls_reject_malformed_unbracketed_hosts() {
        for value in [
            "https://exa[mple.com",
            "https://exa]mple.com",
            "https://a..b.example",
            "https://-a.example",
            "https://a-.example",
            "https://a_b.example",
        ] {
            assert!(
                matches!(
                    validate_public_url_with_host_policy(
                        "PUBLIC_WEB_BASE",
                        value,
                        PublicUrlKind::Http,
                        EnvKind::Production,
                        HostPolicy::production(),
                    ),
                    Err(StaticWebUrlPolicyError::Url(
                        StaticWebError::InvalidUrl { .. }
                    ))
                ),
                "{value} should be rejected as an invalid URL"
            );
        }
    }

    #[test]
    fn static_header_files_parse_name_value_lines() {
        let headers = parse_static_headers(
            r#"
# comment
Content-Security-Policy: default-src 'self'
X-Frame-Options: DENY
malformed
"#,
        );
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[0].name, "Content-Security-Policy");
        assert_eq!(headers[0].value, "default-src 'self'");
    }

    #[test]
    fn development_urls_allow_insecure_schemes_and_websocket_kind() {
        let url = validate_public_url(
            "PUBLIC_WEB_BASE",
            " HTTP://Example.com/path ",
            PublicUrlKind::Http,
            EnvKind::Development,
        )
        .unwrap();
        assert_eq!(url.scheme, "http");
        assert_eq!(url.remainder, "Example.com/path");

        assert!(validate_public_url(
            "PUBLIC_WS_BASE",
            "wss://example.com/socket",
            PublicUrlKind::WebSocket,
            EnvKind::Production,
        )
        .is_ok());
        assert!(matches!(
            validate_public_url(
                "PUBLIC_WS_BASE",
                "ws://example.com/socket",
                PublicUrlKind::WebSocket,
                EnvKind::Staging,
            ),
            Err(StaticWebError::InsecureScheme { scheme, .. }) if scheme == "ws"
        ));
    }

    #[test]
    fn host_policy_errors_preserve_rejection_reasons() {
        let error = validate_public_url_with_host_policy(
            "PUBLIC_WEB_BASE",
            "https://app.localhost",
            PublicUrlKind::Http,
            EnvKind::Production,
            HostPolicy::production(),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "PUBLIC_WEB_BASE host app.localhost is rejected: localhost host"
        );

        let error = validate_public_url_with_host_policy(
            "PUBLIC_WEB_BASE",
            "https://0x7f000001",
            PublicUrlKind::Http,
            EnvKind::Production,
            HostPolicy::production(),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            StaticWebUrlPolicyError::RejectedHost {
                reason: "non-canonical IPv4 host",
                ..
            }
        ));
        assert!(validate_public_url_with_host_policy(
            "PUBLIC_WEB_BASE",
            "https://0x7f000001",
            PublicUrlKind::Http,
            EnvKind::Production,
            HostPolicy::permissive(),
        )
        .is_ok());
    }

    #[test]
    fn header_directives_report_missing_header_and_missing_directive() {
        let headers = [(
            "content-security-policy",
            "default-src 'self'; frame-ancestors 'none'",
        )];
        assert!(
            assert_header_directives(headers, "Content-Security-Policy", &["default-src"]).is_ok()
        );
        assert!(assert_header_directives(
            headers,
            "Content-Security-Policy",
            &["frame-ancestors 'none'"]
        )
        .is_ok());
        assert_eq!(
            assert_header_directives(
                [("Content-Security-Policy", "not-frame-ancestors 'none'")],
                "Content-Security-Policy",
                &["frame-ancestors"]
            )
            .unwrap_err(),
            StaticWebError::MissingHeaderDirective {
                name: "Content-Security-Policy".to_owned(),
                directive: "frame-ancestors".to_owned()
            }
        );
        assert_eq!(
            assert_header_directives(headers, "X-Frame-Options", &["DENY"]).unwrap_err(),
            StaticWebError::MissingHeader {
                name: "X-Frame-Options".to_owned()
            }
        );
        assert_eq!(
            assert_header_directives(headers, "Content-Security-Policy", &["script-src"])
                .unwrap_err(),
            StaticWebError::MissingHeaderDirective {
                name: "Content-Security-Policy".to_owned(),
                directive: "script-src".to_owned()
            }
        );
        assert!(assert_header_directives(headers, "Content-Security-Policy", &[""]).is_err());
    }

    #[test]
    fn static_web_error_display_is_stable() {
        assert_eq!(
            StaticWebError::Empty {
                name: "PUBLIC_WEB_BASE".to_owned()
            }
            .to_string(),
            "PUBLIC_WEB_BASE is empty"
        );
        assert_eq!(
            StaticWebError::InvalidUrl {
                name: "PUBLIC_WEB_BASE".to_owned()
            }
            .to_string(),
            "PUBLIC_WEB_BASE is not a valid public URL"
        );
        assert_eq!(
            StaticWebError::SecretLikeValue {
                key: "SECRET".to_owned(),
                reason: "secret-like key name",
            }
            .to_string(),
            "public config key SECRET looks secret-like: secret-like key name"
        );
    }
}
