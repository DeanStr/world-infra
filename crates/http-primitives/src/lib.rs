//! Framework-neutral HTTP request parsing helpers.

use std::{
    error::Error,
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
};

/// Error returned when parsing HTTP primitives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpPrimitiveError {
    /// A value was empty after trimming.
    Empty,
    /// A request id contained invalid characters.
    InvalidRequestId,
    /// An origin was malformed.
    InvalidOrigin,
    /// A CIDR prefix was malformed.
    InvalidCidr,
    /// An IP address was malformed.
    InvalidIp,
}

impl fmt::Display for HttpPrimitiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("value is empty"),
            Self::InvalidRequestId => f.write_str("request id is invalid"),
            Self::InvalidOrigin => f.write_str("origin is invalid"),
            Self::InvalidCidr => f.write_str("CIDR is invalid"),
            Self::InvalidIp => f.write_str("IP address is invalid"),
        }
    }
}

impl Error for HttpPrimitiveError {}

/// Normalized request identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RequestId(String);

impl RequestId {
    /// Validate and normalize a request id.
    ///
    /// # Errors
    ///
    /// Returns [`HttpPrimitiveError`] if the value is empty or contains
    /// unsupported characters.
    pub fn new(value: impl AsRef<str>) -> Result<Self, HttpPrimitiveError> {
        let value = value.as_ref();
        if value.chars().any(char::is_control) {
            return Err(HttpPrimitiveError::InvalidRequestId);
        }
        let value = value.trim();
        if value.is_empty() {
            return Err(HttpPrimitiveError::Empty);
        }
        if value.len() > 128 || value.chars().any(|ch| !ch.is_ascii_graphic()) {
            return Err(HttpPrimitiveError::InvalidRequestId);
        }
        Ok(Self(value.to_owned()))
    }

    /// Standard request id header name.
    #[must_use]
    pub const fn header_name() -> &'static str {
        "x-request-id"
    }

    /// Access the normalized id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Parsed HTTP origin.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Origin {
    /// Lowercase scheme.
    pub scheme: String,
    /// Lowercase host.
    pub host: String,
    /// Explicit port, if present.
    pub port: Option<u16>,
}

impl Origin {
    /// Parse an origin such as `https://example.com:443`.
    ///
    /// # Errors
    ///
    /// Returns [`HttpPrimitiveError::InvalidOrigin`] for malformed values.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, HttpPrimitiveError> {
        let value = value.as_ref().trim();
        let (scheme, rest) = value
            .split_once("://")
            .ok_or(HttpPrimitiveError::InvalidOrigin)?;
        let scheme = scheme.to_ascii_lowercase();
        if !matches!(scheme.as_str(), "http" | "https" | "ws" | "wss") {
            return Err(HttpPrimitiveError::InvalidOrigin);
        }
        if rest.is_empty() || rest.contains('/') {
            return Err(HttpPrimitiveError::InvalidOrigin);
        }
        let (host, port) = parse_host_port(rest)?;
        Ok(Self {
            scheme,
            host: host.to_ascii_lowercase(),
            port,
        })
    }

    /// Return whether this is a local development origin.
    #[must_use]
    pub fn is_local(&self) -> bool {
        matches!(self.host.as_str(), "localhost" | "127.0.0.1" | "::1")
    }
}

fn parse_host_port(value: &str) -> Result<(String, Option<u16>), HttpPrimitiveError> {
    if value.starts_with('[') {
        let (host, tail) = value
            .split_once(']')
            .ok_or(HttpPrimitiveError::InvalidOrigin)?;
        let port = if tail.is_empty() {
            None
        } else {
            Some(
                tail.strip_prefix(':')
                    .ok_or(HttpPrimitiveError::InvalidOrigin)?
                    .parse::<u16>()
                    .map_err(|_| HttpPrimitiveError::InvalidOrigin)?,
            )
        };
        let host = host
            .strip_prefix('[')
            .ok_or(HttpPrimitiveError::InvalidOrigin)?;
        if host.is_empty() || host.starts_with('[') || host.parse::<Ipv6Addr>().is_err() {
            return Err(HttpPrimitiveError::InvalidOrigin);
        }
        return Ok((host.to_owned(), port));
    }
    if let Some((host, port)) = value.rsplit_once(':') {
        if host.contains(':') {
            if value.parse::<Ipv6Addr>().is_ok() {
                return Ok((value.to_owned(), None));
            }
            return Err(HttpPrimitiveError::InvalidOrigin);
        }
        if !valid_unbracketed_origin_host(host) {
            return Err(HttpPrimitiveError::InvalidOrigin);
        }
        let port = port
            .parse::<u16>()
            .map_err(|_| HttpPrimitiveError::InvalidOrigin)?;
        return Ok((host.to_owned(), Some(port)));
    }
    if !valid_unbracketed_origin_host(value) {
        return Err(HttpPrimitiveError::InvalidOrigin);
    }
    Ok((value.to_owned(), None))
}

fn valid_unbracketed_origin_host(host: &str) -> bool {
    !host.is_empty()
        && !host.starts_with('.')
        && !host.ends_with('.')
        && !host.contains("..")
        && host
            .chars()
            .all(|ch| ch.is_ascii_graphic() && !matches!(ch, '/' | '?' | '#' | '@' | '[' | ']'))
}

/// Parse comma-separated allowed origins.
///
/// # Errors
///
/// Returns [`HttpPrimitiveError::InvalidOrigin`] if any origin is malformed.
pub fn parse_allowed_origins(value: impl AsRef<str>) -> Result<Vec<Origin>, HttpPrimitiveError> {
    value
        .as_ref()
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(Origin::parse)
        .collect()
}

/// A trusted proxy network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cidr {
    addr: IpAddr,
    prefix: u8,
}

impl Cidr {
    /// Construct a single-IP trusted proxy network.
    #[must_use]
    pub const fn single_ip(addr: IpAddr) -> Self {
        let prefix = match addr {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        Self { addr, prefix }
    }

    /// Parse CIDR notation.
    ///
    /// # Errors
    ///
    /// Returns [`HttpPrimitiveError::InvalidCidr`] for malformed prefixes.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, HttpPrimitiveError> {
        let value = value.as_ref().trim();
        let (addr, prefix) = value
            .split_once('/')
            .ok_or(HttpPrimitiveError::InvalidCidr)?;
        let addr = addr
            .parse::<IpAddr>()
            .map_err(|_| HttpPrimitiveError::InvalidIp)?;
        let prefix = prefix
            .parse::<u8>()
            .map_err(|_| HttpPrimitiveError::InvalidCidr)?;
        let max = match addr {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        if prefix > max {
            return Err(HttpPrimitiveError::InvalidCidr);
        }
        Ok(Self { addr, prefix })
    }

    /// Return whether this CIDR contains an address.
    #[must_use]
    pub fn contains(self, addr: IpAddr) -> bool {
        match (self.addr, addr) {
            (IpAddr::V4(network), IpAddr::V4(addr)) => {
                let mask = if self.prefix == 0 {
                    0
                } else {
                    u32::MAX << (32 - self.prefix)
                };
                u32::from(network) & mask == u32::from(addr) & mask
            }
            (IpAddr::V6(network), IpAddr::V6(addr)) => {
                let mask = if self.prefix == 0 {
                    0
                } else {
                    u128::MAX << (128 - self.prefix)
                };
                u128::from(network) & mask == u128::from(addr) & mask
            }
            _ => false,
        }
    }

    /// Return the network address stored in this CIDR.
    #[must_use]
    pub const fn addr(self) -> IpAddr {
        self.addr
    }

    /// Return the prefix length.
    #[must_use]
    pub const fn prefix(self) -> u8 {
        self.prefix
    }
}

/// Product-provided trusted proxy configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrustedProxyConfig {
    trusted_proxies: Vec<Cidr>,
}

impl TrustedProxyConfig {
    /// Construct a config from trusted proxy CIDRs.
    #[must_use]
    pub fn new(trusted_proxies: impl Into<Vec<Cidr>>) -> Self {
        Self {
            trusted_proxies: trusted_proxies.into(),
        }
    }

    /// Construct a config that trusts only one peer IP.
    #[must_use]
    pub fn single_peer(peer: IpAddr) -> Self {
        Self::new(vec![Cidr::single_ip(peer)])
    }

    /// Parse a comma-separated CIDR list.
    ///
    /// # Errors
    ///
    /// Returns [`HttpPrimitiveError`] if any CIDR is malformed.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, HttpPrimitiveError> {
        parse_trusted_proxies(value).map(Self::new)
    }

    /// Return whether the peer is trusted.
    #[must_use]
    pub fn trusts_peer(&self, peer: IpAddr) -> bool {
        self.trusted_proxies
            .iter()
            .any(|proxy| proxy.contains(peer))
    }

    /// Access the trusted proxy CIDRs.
    #[must_use]
    pub fn trusted_proxies(&self) -> &[Cidr] {
        &self.trusted_proxies
    }
}

/// Parse a comma-separated CIDR list.
///
/// # Errors
///
/// Returns [`HttpPrimitiveError`] if any CIDR is malformed.
pub fn parse_trusted_proxies(value: impl AsRef<str>) -> Result<Vec<Cidr>, HttpPrimitiveError> {
    value
        .as_ref()
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(Cidr::parse)
        .collect()
}

/// Extract the client IP using proxy headers only when the peer is trusted.
#[must_use]
pub fn extract_client_ip(
    peer: IpAddr,
    trusted_proxies: &[Cidr],
    forwarded: Option<&str>,
    x_forwarded_for: Option<&str>,
    x_real_ip: Option<&str>,
) -> IpAddr {
    if !trusted_proxies.iter().any(|proxy| proxy.contains(peer)) {
        return peer;
    }
    match parse_forwarded_for(forwarded, trusted_proxies) {
        HeaderClientIp::Found(addr) => return addr,
        HeaderClientIp::Invalid => return peer,
        HeaderClientIp::Missing => {}
    }
    match parse_x_forwarded_for(x_forwarded_for, trusted_proxies) {
        HeaderClientIp::Found(addr) => return addr,
        HeaderClientIp::Invalid => return peer,
        HeaderClientIp::Missing => {}
    }
    match parse_single_ip_header(x_real_ip) {
        HeaderClientIp::Found(addr) => addr,
        HeaderClientIp::Invalid | HeaderClientIp::Missing => peer,
    }
}

/// Extract the client IP using a trusted proxy config.
#[must_use]
pub fn client_ip_from_headers(
    peer: IpAddr,
    config: &TrustedProxyConfig,
    forwarded: Option<&str>,
    x_forwarded_for: Option<&str>,
    x_real_ip: Option<&str>,
) -> IpAddr {
    extract_client_ip(
        peer,
        config.trusted_proxies(),
        forwarded,
        x_forwarded_for,
        x_real_ip,
    )
}

/// Extract the client IP from an [`http::HeaderMap`].
///
/// This helper is feature-gated so framework-neutral users do not depend on
/// the `http` crate unless they opt in.
#[cfg(feature = "http")]
#[must_use]
pub fn client_ip_from_header_map(
    headers: &http::HeaderMap,
    peer: IpAddr,
    config: &TrustedProxyConfig,
) -> IpAddr {
    let forwarded = joined_header_values(headers, "forwarded");
    let x_forwarded_for = joined_header_values(headers, "x-forwarded-for");
    let x_real_ip = joined_header_values(headers, "x-real-ip");
    client_ip_from_headers(
        peer,
        config,
        forwarded.as_deref(),
        x_forwarded_for.as_deref(),
        x_real_ip.as_deref(),
    )
}

#[cfg(feature = "http")]
fn joined_header_values(headers: &http::HeaderMap, name: &'static str) -> Option<String> {
    let mut values = headers.get_all(name).iter();
    let first = values.next()?;
    let mut joined = first
        .to_str()
        .map_or_else(|_| "__invalid_header_value__".to_owned(), ToOwned::to_owned);
    for value in values {
        joined.push(',');
        joined.push_str(value.to_str().unwrap_or("__invalid_header_value__"));
    }
    Some(joined)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeaderClientIp {
    Missing,
    Invalid,
    Found(IpAddr),
}

fn parse_forwarded_for(value: Option<&str>, trusted_proxies: &[Cidr]) -> HeaderClientIp {
    let Some(value) = value else {
        return HeaderClientIp::Missing;
    };
    let mut saw_entry = false;
    for entry in value
        .split(',')
        .rev()
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        saw_entry = true;
        let addr = match parse_forwarded_entry(entry) {
            HeaderClientIp::Found(addr) => addr,
            HeaderClientIp::Invalid => return HeaderClientIp::Invalid,
            HeaderClientIp::Missing => return HeaderClientIp::Invalid,
        };
        if !is_trusted_proxy(addr, trusted_proxies) {
            return HeaderClientIp::Found(addr);
        }
    }
    if saw_entry {
        HeaderClientIp::Invalid
    } else {
        HeaderClientIp::Missing
    }
}

fn parse_forwarded_entry(entry: &str) -> HeaderClientIp {
    for part in entry.split(';') {
        let Some((name, raw)) = part.trim().split_once('=') else {
            continue;
        };
        if name.eq_ignore_ascii_case("for") {
            return parse_ip_token(raw.trim_matches('"'))
                .map_or(HeaderClientIp::Invalid, HeaderClientIp::Found);
        }
    }
    HeaderClientIp::Invalid
}

fn parse_x_forwarded_for(value: Option<&str>, trusted_proxies: &[Cidr]) -> HeaderClientIp {
    let Some(value) = value else {
        return HeaderClientIp::Missing;
    };
    let mut saw_entry = false;
    for entry in value
        .split(',')
        .rev()
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        saw_entry = true;
        let Some(addr) = parse_ip_token(entry) else {
            return HeaderClientIp::Invalid;
        };
        if !is_trusted_proxy(addr, trusted_proxies) {
            return HeaderClientIp::Found(addr);
        }
    }
    if saw_entry {
        HeaderClientIp::Invalid
    } else {
        HeaderClientIp::Missing
    }
}

fn parse_single_ip_header(value: Option<&str>) -> HeaderClientIp {
    let Some(value) = value else {
        return HeaderClientIp::Missing;
    };
    parse_ip_token(value).map_or(HeaderClientIp::Invalid, HeaderClientIp::Found)
}

fn parse_ip_token(value: &str) -> Option<IpAddr> {
    let value = value.trim().trim_matches('"');
    if value.starts_with('[') {
        let (host, tail) = value.split_once(']')?;
        let host = host.strip_prefix('[')?;
        if host.is_empty() || host.starts_with('[') {
            return None;
        }
        if !tail.is_empty() {
            let port = tail.strip_prefix(':')?;
            port.parse::<u16>().ok()?;
        }
        return host.parse().ok();
    }
    value
        .parse::<IpAddr>()
        .ok()
        .or_else(|| value.parse::<SocketAddr>().ok().map(|socket| socket.ip()))
}

fn is_trusted_proxy(addr: IpAddr, trusted_proxies: &[Cidr]) -> bool {
    trusted_proxies.iter().any(|proxy| proxy.contains(addr))
}

/// Return true when a bind address is publicly reachable.
#[must_use]
pub fn is_public_bind(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(addr) => addr == Ipv4Addr::UNSPECIFIED || is_public_unicast_v4(addr),
        IpAddr::V6(addr) => addr == Ipv6Addr::UNSPECIFIED || is_public_unicast_v6(addr),
    }
}

fn is_public_unicast_v4(addr: Ipv4Addr) -> bool {
    !(addr.octets()[0] == 0
        || addr.is_loopback()
        || addr.is_private()
        || addr.is_link_local()
        || addr.is_multicast()
        || addr.is_broadcast()
        || is_shared_v4(addr)
        || is_benchmarking_v4(addr)
        || is_documentation_v4(addr)
        || is_reserved_v4(addr))
}

fn is_shared_v4(addr: Ipv4Addr) -> bool {
    let [first, second, _, _] = addr.octets();
    first == 100 && (64..=127).contains(&second)
}

fn is_benchmarking_v4(addr: Ipv4Addr) -> bool {
    let [first, second, _, _] = addr.octets();
    first == 198 && matches!(second, 18 | 19)
}

fn is_documentation_v4(addr: Ipv4Addr) -> bool {
    let [first, second, third, _] = addr.octets();
    matches!(
        (first, second, third),
        (192, 0, 2) | (198, 51, 100) | (203, 0, 113)
    )
}

fn is_reserved_v4(addr: Ipv4Addr) -> bool {
    addr.octets()[0] >= 240
}

fn is_public_unicast_v6(addr: Ipv6Addr) -> bool {
    if let Some(addr) = addr.to_ipv4_mapped() {
        return is_public_unicast_v4(addr);
    }

    !(addr.is_unspecified()
        || addr.is_loopback()
        || addr.is_multicast()
        || is_unique_local_v6(addr)
        || is_unicast_link_local_v6(addr)
        || is_site_local_v6(addr)
        || is_documentation_v6(addr))
}

fn is_unique_local_v6(addr: Ipv6Addr) -> bool {
    (addr.segments()[0] & 0xfe00) == 0xfc00
}

fn is_unicast_link_local_v6(addr: Ipv6Addr) -> bool {
    (addr.segments()[0] & 0xffc0) == 0xfe80
}

fn is_site_local_v6(addr: Ipv6Addr) -> bool {
    (addr.segments()[0] & 0xffc0) == 0xfec0
}

fn is_documentation_v6(addr: Ipv6Addr) -> bool {
    let segments = addr.segments();
    segments[0] == 0x2001 && segments[1] == 0x0db8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_ids_are_strict() {
        assert_eq!(RequestId::new(" req-1 ").unwrap().as_str(), "req-1");
        assert!(RequestId::new("bad id").is_err());
        assert!(RequestId::new("req-1\n").is_err());
    }

    #[test]
    fn cidr_matches_addresses() {
        let cidr = Cidr::parse("10.0.0.0/8").unwrap();
        assert!(cidr.contains("10.1.2.3".parse().unwrap()));
        assert!(!cidr.contains("11.1.2.3".parse().unwrap()));
        let single = Cidr::single_ip("10.1.2.3".parse().unwrap());
        assert!(single.contains("10.1.2.3".parse().unwrap()));
        assert!(!single.contains("10.1.2.4".parse().unwrap()));
    }

    #[test]
    fn trusted_proxy_unlocks_forwarded_headers() {
        let config = TrustedProxyConfig::parse("10.0.0.0/8").unwrap();
        let client = client_ip_from_headers(
            "10.1.1.1".parse().unwrap(),
            &config,
            Some(r#"for="203.0.113.10";proto=https"#),
            None,
            None,
        );
        assert_eq!(client, "203.0.113.10".parse::<IpAddr>().unwrap());
        let untrusted = extract_client_ip(
            "198.51.100.1".parse().unwrap(),
            config.trusted_proxies(),
            Some(r#"for="203.0.113.10""#),
            None,
            None,
        );
        assert_eq!(untrusted, "198.51.100.1".parse::<IpAddr>().unwrap());
    }

    #[cfg(feature = "http")]
    #[test]
    fn header_map_combines_repeated_forwarded_values() {
        let config = TrustedProxyConfig::parse("10.0.0.0/8").unwrap();
        let mut headers = http::HeaderMap::new();
        headers.append(
            "forwarded",
            http::HeaderValue::from_static("for=198.51.100.200"),
        );
        headers.append(
            "forwarded",
            http::HeaderValue::from_static("for=203.0.113.44, for=10.1.1.1"),
        );

        let client = client_ip_from_header_map(&headers, "10.1.1.1".parse().unwrap(), &config);
        assert_eq!(client, "203.0.113.44".parse::<IpAddr>().unwrap());
    }

    #[cfg(feature = "http")]
    #[test]
    fn header_map_combines_repeated_x_forwarded_for_values() {
        let config = TrustedProxyConfig::parse("10.0.0.0/8").unwrap();
        let mut headers = http::HeaderMap::new();
        headers.append(
            "x-forwarded-for",
            http::HeaderValue::from_static("198.51.100.200"),
        );
        headers.append(
            "x-forwarded-for",
            http::HeaderValue::from_static("203.0.113.44, 10.1.1.1"),
        );

        let client = client_ip_from_header_map(&headers, "10.1.1.1".parse().unwrap(), &config);
        assert_eq!(client, "203.0.113.44".parse::<IpAddr>().unwrap());
    }

    #[cfg(feature = "http")]
    #[test]
    fn header_map_invalid_repeated_header_value_falls_back_to_peer() {
        let config = TrustedProxyConfig::parse("10.0.0.0/8").unwrap();
        let mut headers = http::HeaderMap::new();
        headers.append(
            "x-forwarded-for",
            http::HeaderValue::from_static("203.0.113.44"),
        );
        headers.append(
            "x-forwarded-for",
            http::HeaderValue::from_bytes(b"\xff").unwrap(),
        );

        let client = client_ip_from_header_map(&headers, "10.1.1.1".parse().unwrap(), &config);
        assert_eq!(client, "10.1.1.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn malformed_bracketed_forwarded_ip_falls_back_to_peer() {
        let proxies = parse_trusted_proxies("10.0.0.0/8").unwrap();
        let client = extract_client_ip(
            "10.1.1.1".parse().unwrap(),
            &proxies,
            Some(r#"for="[2001:db8::1]junk";proto=https"#),
            None,
            None,
        );
        assert_eq!(client, "10.1.1.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn forwarded_walks_from_trusted_proxy_side() {
        let proxies = parse_trusted_proxies("10.0.0.0/8").unwrap();
        let client = extract_client_ip(
            "10.1.1.1".parse().unwrap(),
            &proxies,
            Some(r#"for="1.2.3.4";proto=https, for="203.0.113.10""#),
            Some("203.0.113.11"),
            None,
        );
        assert_eq!(client, "203.0.113.10".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn forwarded_skips_trusted_proxy_hops() {
        let proxies = parse_trusted_proxies("10.0.0.0/8").unwrap();
        let client = extract_client_ip(
            "10.1.1.1".parse().unwrap(),
            &proxies,
            Some(r#"for="203.0.113.10";proto=https, for="10.2.2.2""#),
            None,
            None,
        );
        assert_eq!(client, "203.0.113.10".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn malformed_trusted_side_forwarded_ip_falls_back_to_peer() {
        let proxies = parse_trusted_proxies("10.0.0.0/8").unwrap();
        let client = extract_client_ip(
            "10.1.1.1".parse().unwrap(),
            &proxies,
            Some(r#"for="203.0.113.10";proto=https, for="not-an-ip""#),
            Some("203.0.113.11"),
            None,
        );
        assert_eq!(client, "10.1.1.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn forwarded_trusted_side_element_without_for_falls_back_to_peer() {
        let proxies = parse_trusted_proxies("10.0.0.0/8").unwrap();
        let client = extract_client_ip(
            "10.1.1.1".parse().unwrap(),
            &proxies,
            Some(r#"for="203.0.113.10", proto=https"#),
            Some("203.0.113.11"),
            None,
        );
        assert_eq!(client, "10.1.1.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn malformed_bracketed_x_forwarded_for_falls_back_to_peer() {
        let proxies = parse_trusted_proxies("10.0.0.0/8").unwrap();
        let client = extract_client_ip(
            "10.1.1.1".parse().unwrap(),
            &proxies,
            None,
            Some("[2001:db8::1]junk"),
            None,
        );
        assert_eq!(client, "10.1.1.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn nested_bracketed_x_forwarded_for_falls_back_to_peer() {
        let proxies = parse_trusted_proxies("10.0.0.0/8").unwrap();
        let client = extract_client_ip(
            "10.1.1.1".parse().unwrap(),
            &proxies,
            None,
            Some("[[8.8.8.8]"),
            Some("8.8.4.4"),
        );
        assert_eq!(client, "10.1.1.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn x_forwarded_for_walks_from_trusted_proxy_side() {
        let proxies = parse_trusted_proxies("10.0.0.0/8").unwrap();
        let client = extract_client_ip(
            "10.1.1.1".parse().unwrap(),
            &proxies,
            None,
            Some("1.2.3.4, 203.0.113.10"),
            Some("203.0.113.11"),
        );
        assert_eq!(client, "203.0.113.10".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn x_forwarded_for_skips_trusted_proxy_hops() {
        let proxies = parse_trusted_proxies("10.0.0.0/8").unwrap();
        let client = extract_client_ip(
            "10.1.1.1".parse().unwrap(),
            &proxies,
            None,
            Some("203.0.113.10, 10.2.2.2"),
            None,
        );
        assert_eq!(client, "203.0.113.10".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn malformed_trusted_side_x_forwarded_for_falls_back_to_peer() {
        let proxies = parse_trusted_proxies("10.0.0.0/8").unwrap();
        let client = extract_client_ip(
            "10.1.1.1".parse().unwrap(),
            &proxies,
            None,
            Some("203.0.113.10, not-an-ip"),
            Some("203.0.113.11"),
        );
        assert_eq!(client, "10.1.1.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn malformed_bracketed_x_real_ip_falls_back_to_peer() {
        let proxies = parse_trusted_proxies("10.0.0.0/8").unwrap();
        let client = extract_client_ip(
            "10.1.1.1".parse().unwrap(),
            &proxies,
            None,
            None,
            Some("[2001:db8::1]:bad"),
        );
        assert_eq!(client, "10.1.1.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn valid_bracketed_forwarded_ip_with_port_is_allowed() {
        let proxies = parse_trusted_proxies("10.0.0.0/8").unwrap();
        let client = extract_client_ip(
            "10.1.1.1".parse().unwrap(),
            &proxies,
            Some(r#"for="[2001:db8::1]:443";proto=https"#),
            None,
            None,
        );
        assert_eq!(client, "2001:db8::1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn nested_bracketed_forwarded_ip_falls_back_to_peer() {
        let proxies = parse_trusted_proxies("10.0.0.0/8").unwrap();
        let client = extract_client_ip(
            "10.1.1.1".parse().unwrap(),
            &proxies,
            Some(r#"for="[[8.8.8.8]";proto=https"#),
            None,
            None,
        );
        assert_eq!(client, "10.1.1.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn parses_allowed_origins() {
        let origins = parse_allowed_origins(
            "https://example.com, http://localhost:5173, https://2001:db8::1",
        )
        .unwrap();
        assert_eq!(origins.len(), 3);
        assert!(origins[1].is_local());
        assert_eq!(origins[2].host, "2001:db8::1");
    }

    #[test]
    fn origin_schemes_are_case_insensitive() {
        let origin = Origin::parse("HTTPS://Example.COM:443").unwrap();
        assert_eq!(origin.scheme, "https");
        assert_eq!(origin.host, "example.com");
        assert_eq!(origin.port, Some(443));
    }

    #[test]
    fn rejects_malformed_bracketed_origins() {
        assert!(parse_allowed_origins("https://[2001:db8::1]junk").is_err());
        assert!(parse_allowed_origins("https://[2001:db8::1]:bad").is_err());
        assert!(parse_allowed_origins("https://[]:443").is_err());
        assert!(parse_allowed_origins("https://[not-an-ip]:443").is_err());
        assert!(parse_allowed_origins("https://[[2001:db8::1]:443").is_err());
        assert!(parse_allowed_origins("https://[[2001:db8::1]]:443").is_err());
        assert!(parse_allowed_origins("https://[2001:db8::1]:443").is_ok());
    }

    #[test]
    fn rejects_ambiguous_multi_colon_origins() {
        assert!(parse_allowed_origins("https://example.com:bad:tail").is_err());
        assert!(parse_allowed_origins("https://2001:db8::1:443").is_ok());
    }

    #[test]
    fn rejects_empty_origin_hosts() {
        assert!(parse_allowed_origins("https://:443").is_err());
        assert!(parse_allowed_origins("https://").is_err());
    }

    #[test]
    fn rejects_malformed_origin_hosts() {
        assert!(parse_allowed_origins("https://example.com?x").is_err());
        assert!(parse_allowed_origins("https://example.com#frag").is_err());
        assert!(parse_allowed_origins("https://user@example.com").is_err());
        assert!(parse_allowed_origins("https://exa mple.com").is_err());
        assert!(parse_allowed_origins("https://exa\tmple.com").is_err());
        assert!(parse_allowed_origins("https://.example.com").is_err());
        assert!(parse_allowed_origins("https://example.com.").is_err());
        assert!(parse_allowed_origins("https://example..com").is_err());
    }

    #[test]
    fn public_bind_excludes_non_routable_addresses() {
        assert!(is_public_bind("0.0.0.0".parse().unwrap()));
        assert!(is_public_bind("::".parse().unwrap()));
        assert!(is_public_bind("8.8.8.8".parse().unwrap()));
        assert!(is_public_bind("2001:4860:4860::8888".parse().unwrap()));

        assert!(!is_public_bind("127.0.0.1".parse().unwrap()));
        assert!(!is_public_bind("0.1.2.3".parse().unwrap()));
        assert!(!is_public_bind("10.0.0.1".parse().unwrap()));
        assert!(!is_public_bind("169.254.1.10".parse().unwrap()));
        assert!(!is_public_bind("224.0.0.1".parse().unwrap()));
        assert!(!is_public_bind("192.0.2.1".parse().unwrap()));
        assert!(!is_public_bind("fe80::1".parse().unwrap()));
        assert!(!is_public_bind("fec0::1".parse().unwrap()));
        assert!(!is_public_bind("ff02::1".parse().unwrap()));
        assert!(!is_public_bind("2001:db8::1".parse().unwrap()));
        assert!(!is_public_bind("::ffff:127.0.0.1".parse().unwrap()));
        assert!(!is_public_bind("::ffff:10.0.0.1".parse().unwrap()));
        assert!(is_public_bind("::ffff:8.8.8.8".parse().unwrap()));
    }
}
