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
        let value = value.as_ref().trim();
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
        if !matches!(scheme, "http" | "https" | "ws" | "wss") {
            return Err(HttpPrimitiveError::InvalidOrigin);
        }
        if rest.is_empty() || rest.contains('/') {
            return Err(HttpPrimitiveError::InvalidOrigin);
        }
        let (host, port) = parse_host_port(rest)?;
        Ok(Self {
            scheme: scheme.to_ascii_lowercase(),
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
        let host = host.trim_start_matches('[');
        if host.is_empty() || host.parse::<Ipv6Addr>().is_err() {
            return Err(HttpPrimitiveError::InvalidOrigin);
        }
        return Ok((host.to_owned(), port));
    }
    if let Some((host, port)) = value.rsplit_once(':') {
        if host.contains(':') {
            return Ok((value.to_owned(), None));
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
    !host.is_empty() && !host.contains(['[', ']'])
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
    match parse_forwarded_for(forwarded) {
        HeaderClientIp::Found(addr) => return addr,
        HeaderClientIp::Invalid => return peer,
        HeaderClientIp::Missing => {}
    }
    match parse_x_forwarded_for(x_forwarded_for) {
        HeaderClientIp::Found(addr) => return addr,
        HeaderClientIp::Invalid => return peer,
        HeaderClientIp::Missing => {}
    }
    match parse_single_ip_header(x_real_ip) {
        HeaderClientIp::Found(addr) => addr,
        HeaderClientIp::Invalid | HeaderClientIp::Missing => peer,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeaderClientIp {
    Missing,
    Invalid,
    Found(IpAddr),
}

fn parse_forwarded_for(value: Option<&str>) -> HeaderClientIp {
    let Some(value) = value else {
        return HeaderClientIp::Missing;
    };
    let Some(entry) = value.split(',').next() else {
        return HeaderClientIp::Missing;
    };
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

fn parse_x_forwarded_for(value: Option<&str>) -> HeaderClientIp {
    let Some(value) = value else {
        return HeaderClientIp::Missing;
    };
    let Some(first) = value.split(',').next() else {
        return HeaderClientIp::Missing;
    };
    parse_ip_token(first).map_or(HeaderClientIp::Invalid, HeaderClientIp::Found)
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
        if !tail.is_empty() {
            let port = tail.strip_prefix(':')?;
            port.parse::<u16>().ok()?;
        }
        return host.trim_start_matches('[').parse().ok();
    }
    value
        .parse::<IpAddr>()
        .ok()
        .or_else(|| value.parse::<SocketAddr>().ok().map(|socket| socket.ip()))
}

/// Return true when a bind address is publicly reachable.
#[must_use]
pub fn is_public_bind(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(addr) => {
            addr == Ipv4Addr::UNSPECIFIED || !(addr.is_loopback() || addr.is_private())
        }
        IpAddr::V6(addr) => {
            addr == Ipv6Addr::UNSPECIFIED || !(addr.is_loopback() || is_unique_local_v6(addr))
        }
    }
}

fn is_unique_local_v6(addr: Ipv6Addr) -> bool {
    (addr.segments()[0] & 0xfe00) == 0xfc00
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_ids_are_strict() {
        assert_eq!(RequestId::new(" req-1 ").unwrap().as_str(), "req-1");
        assert!(RequestId::new("bad id").is_err());
    }

    #[test]
    fn cidr_matches_addresses() {
        let cidr = Cidr::parse("10.0.0.0/8").unwrap();
        assert!(cidr.contains("10.1.2.3".parse().unwrap()));
        assert!(!cidr.contains("11.1.2.3".parse().unwrap()));
    }

    #[test]
    fn trusted_proxy_unlocks_forwarded_headers() {
        let proxies = parse_trusted_proxies("10.0.0.0/8").unwrap();
        let client = extract_client_ip(
            "10.1.1.1".parse().unwrap(),
            &proxies,
            Some(r#"for="203.0.113.10";proto=https"#),
            None,
            None,
        );
        assert_eq!(client, "203.0.113.10".parse::<IpAddr>().unwrap());
        let untrusted = extract_client_ip(
            "198.51.100.1".parse().unwrap(),
            &proxies,
            Some(r#"for="203.0.113.10""#),
            None,
            None,
        );
        assert_eq!(untrusted, "198.51.100.1".parse::<IpAddr>().unwrap());
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
    fn malformed_first_forwarded_ip_falls_back_to_peer() {
        let proxies = parse_trusted_proxies("10.0.0.0/8").unwrap();
        let client = extract_client_ip(
            "10.1.1.1".parse().unwrap(),
            &proxies,
            Some(r#"for="not-an-ip";proto=https, for="203.0.113.10""#),
            Some("203.0.113.11"),
            None,
        );
        assert_eq!(client, "10.1.1.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn forwarded_first_element_without_for_falls_back_to_peer() {
        let proxies = parse_trusted_proxies("10.0.0.0/8").unwrap();
        let client = extract_client_ip(
            "10.1.1.1".parse().unwrap(),
            &proxies,
            Some(r#"proto=https, for="203.0.113.10""#),
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
    fn malformed_first_x_forwarded_for_falls_back_to_peer() {
        let proxies = parse_trusted_proxies("10.0.0.0/8").unwrap();
        let client = extract_client_ip(
            "10.1.1.1".parse().unwrap(),
            &proxies,
            None,
            Some("not-an-ip, 203.0.113.10"),
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
    fn parses_allowed_origins() {
        let origins = parse_allowed_origins("https://example.com, http://localhost:5173").unwrap();
        assert_eq!(origins.len(), 2);
        assert!(origins[1].is_local());
    }

    #[test]
    fn rejects_malformed_bracketed_origins() {
        assert!(parse_allowed_origins("https://[2001:db8::1]junk").is_err());
        assert!(parse_allowed_origins("https://[2001:db8::1]:bad").is_err());
        assert!(parse_allowed_origins("https://[]:443").is_err());
        assert!(parse_allowed_origins("https://[not-an-ip]:443").is_err());
        assert!(parse_allowed_origins("https://[2001:db8::1]:443").is_ok());
    }

    #[test]
    fn rejects_empty_origin_hosts() {
        assert!(parse_allowed_origins("https://:443").is_err());
        assert!(parse_allowed_origins("https://").is_err());
    }
}
