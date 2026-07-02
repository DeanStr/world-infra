//! WebSocket authentication handshake and control-frame primitives.
//!
//! This crate shares the product-neutral shape of Airline's hardened WebSocket
//! auth posture: require an initial auth frame, bound frame size, reject binary
//! auth frames, return stable auth-error codes, issue an auth nonce, require
//! subscribe/control frames to echo it, and rate-limit control bursts. Products
//! still own token verification, subscription authorization, event filtering,
//! and frame payload schemas.

use std::{
    error::Error,
    fmt,
    time::{Duration, Instant},
};

use serde::Deserialize;

/// WebSocket auth/control helper error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsAuthError {
    /// Authentication is required before subscription/control work.
    AuthenticationRequired,
    /// Text frame exceeded the configured byte limit.
    FrameTooLarge {
        /// Frame length.
        len: usize,
        /// Configured maximum.
        max: usize,
    },
    /// Binary auth frames are not accepted.
    BinaryAuthFrame,
    /// Auth frame was not valid JSON.
    MalformedAuthFrame,
    /// Auth frame did not declare the expected message type.
    InvalidMessageType,
    /// Auth frame token was blank.
    MissingToken,
    /// Control frame did not echo the authenticated nonce.
    InvalidAuthNonce,
    /// Control frame exceeded its rate limit.
    ControlRateLimited,
}

impl WsAuthError {
    /// Stable error code suitable for product-owned wire messages.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::AuthenticationRequired => "authentication_required",
            Self::FrameTooLarge { .. } => "frame_too_large",
            Self::BinaryAuthFrame => "binary_auth_unsupported",
            Self::MalformedAuthFrame => "invalid_auth",
            Self::InvalidMessageType => "invalid_auth",
            Self::MissingToken => "invalid_auth",
            Self::InvalidAuthNonce => "invalid_auth_nonce",
            Self::ControlRateLimited => "control_rate_limited",
        }
    }
}

impl fmt::Display for WsAuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthenticationRequired => f.write_str("authentication required"),
            Self::FrameTooLarge { len, max } => {
                write!(f, "websocket auth frame has {len} bytes; maximum is {max}")
            }
            Self::BinaryAuthFrame => f.write_str("binary authentication frames are unsupported"),
            Self::MalformedAuthFrame => f.write_str("authentication frame is malformed"),
            Self::InvalidMessageType => f.write_str("authentication frame type is invalid"),
            Self::MissingToken => f.write_str("authentication frame token is missing"),
            Self::InvalidAuthNonce => f.write_str("auth nonce is invalid"),
            Self::ControlRateLimited => f.write_str("control frame rate limit exceeded"),
        }
    }
}

impl Error for WsAuthError {}

/// WebSocket auth handshake configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WsAuthConfig {
    /// Maximum auth frame size in bytes.
    pub max_auth_frame_bytes: usize,
    /// Required auth message type.
    pub auth_message_type: &'static str,
}

impl Default for WsAuthConfig {
    fn default() -> Self {
        Self {
            max_auth_frame_bytes: 4096,
            auth_message_type: "auth",
        }
    }
}

/// Parsed initial authentication frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitialAuthFrame {
    /// Bearer or opaque token supplied by the client.
    pub token: String,
}

#[derive(Debug, Deserialize)]
struct RawInitialAuthFrame {
    #[serde(alias = "messageType", alias = "type")]
    message_type: Option<String>,
    token: Option<String>,
}

/// Parse and validate an initial text auth frame.
///
/// # Errors
///
/// Returns [`WsAuthError`] for oversized, malformed, non-auth, or blank-token
/// frames.
pub fn parse_initial_auth_text(
    text: &str,
    config: WsAuthConfig,
) -> Result<InitialAuthFrame, WsAuthError> {
    if text.len() > config.max_auth_frame_bytes {
        return Err(WsAuthError::FrameTooLarge {
            len: text.len(),
            max: config.max_auth_frame_bytes,
        });
    }
    let raw: RawInitialAuthFrame =
        serde_json::from_str(text).map_err(|_| WsAuthError::MalformedAuthFrame)?;
    if raw.message_type.as_deref() != Some(config.auth_message_type) {
        return Err(WsAuthError::InvalidMessageType);
    }
    let token = raw.token.unwrap_or_default();
    if !valid_ws_token(&token) {
        return Err(WsAuthError::MissingToken);
    }
    Ok(InitialAuthFrame { token })
}

fn valid_ws_token(token: &str) -> bool {
    if token.is_empty() || token.trim() != token || token.chars().any(char::is_control) {
        return false;
    }
    if token.chars().any(char::is_whitespace) {
        return auth_primitives::parse_bearer_authorization(Some(token)).is_ok();
    }
    true
}

/// Reject an initial binary auth frame.
///
/// This small helper lets products map binary frames to the same stable error
/// code without copying the decision.
///
/// # Errors
///
/// Always returns [`WsAuthError::BinaryAuthFrame`].
pub fn reject_initial_auth_binary() -> Result<InitialAuthFrame, WsAuthError> {
    Err(WsAuthError::BinaryAuthFrame)
}

/// Authenticated WebSocket nonce.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AuthNonce(String);

impl AuthNonce {
    /// Construct a nonce from product-generated entropy.
    ///
    /// # Errors
    ///
    /// Returns [`WsAuthError::InvalidAuthNonce`] for blank, whitespace-padded,
    /// or control-character values.
    pub fn new(value: impl AsRef<str>) -> Result<Self, WsAuthError> {
        let value = value.as_ref();
        if value.is_empty() || value.trim() != value || value.chars().any(char::is_control) {
            return Err(WsAuthError::InvalidAuthNonce);
        }
        Ok(Self(value.to_owned()))
    }

    /// Access the nonce string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Require a control/subscribe frame to echo the authenticated nonce.
///
/// # Errors
///
/// Returns [`WsAuthError::InvalidAuthNonce`] when the provided nonce is missing
/// or does not match.
pub fn require_auth_nonce(expected: &AuthNonce, provided: Option<&str>) -> Result<(), WsAuthError> {
    match provided {
        Some(value) if value == expected.as_str() => Ok(()),
        _ => Err(WsAuthError::InvalidAuthNonce),
    }
}

/// Sliding-window control-frame limiter.
#[derive(Debug, Clone)]
pub struct ControlFrameRateLimit {
    max_frames: u32,
    window: Duration,
    accepted_at: Vec<Instant>,
}

impl ControlFrameRateLimit {
    /// Construct a control-frame limiter.
    #[must_use]
    pub const fn new(max_frames: u32, window: Duration) -> Self {
        Self {
            max_frames,
            window,
            accepted_at: Vec::new(),
        }
    }

    /// Return whether a control frame is allowed at `now`.
    #[must_use]
    pub fn allow_at(&mut self, now: Instant) -> bool {
        if self.max_frames == 0 || self.window.is_zero() {
            return false;
        }
        self.accepted_at
            .retain(|accepted_at| now.saturating_duration_since(*accepted_at) < self.window);
        if self.accepted_at.len() >= self.max_frames as usize {
            return false;
        }
        self.accepted_at.push(now);
        true
    }

    /// Require that a control frame is allowed at `now`.
    ///
    /// # Errors
    ///
    /// Returns [`WsAuthError::ControlRateLimited`] when the limiter is
    /// exhausted.
    pub fn require_allowed_at(&mut self, now: Instant) -> Result<(), WsAuthError> {
        if self.allow_at(now) {
            Ok(())
        } else {
            Err(WsAuthError::ControlRateLimited)
        }
    }
}

/// Normalize a token from a WebSocket auth frame into a bearer-header value.
///
/// Products that already share HTTP bearer validation can use this to reuse
/// their existing token path after parsing the initial WS auth frame.
#[must_use]
pub fn bearer_header_from_ws_token(token: &str) -> String {
    let token = token.trim();
    if let Ok(credential) = auth_primitives::parse_bearer_authorization(Some(token)) {
        format!("Bearer {}", credential.token())
    } else {
        format!("Bearer {token}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_auth_frame_accepts_camel_case_and_aliases() {
        let frame = parse_initial_auth_text(
            r#"{"messageType":"auth","token":"abc"}"#,
            WsAuthConfig::default(),
        )
        .unwrap();
        assert_eq!(frame.token, "abc");

        let frame = parse_initial_auth_text(
            r#"{"type":"auth","token":"Bearer abc"}"#,
            WsAuthConfig::default(),
        )
        .unwrap();
        assert_eq!(bearer_header_from_ws_token(&frame.token), "Bearer abc");
    }

    #[test]
    fn bearer_header_from_ws_token_canonicalizes_accepted_bearer_shapes() {
        assert_eq!(bearer_header_from_ws_token("bearer   abc"), "Bearer abc");
        assert_eq!(
            bearer_header_from_ws_token("Bearer\u{00a0}abc"),
            "Bearer abc"
        );
    }

    #[test]
    fn initial_auth_frame_rejects_oversized_malformed_and_blank_token() {
        assert_eq!(
            parse_initial_auth_text(
                "{}",
                WsAuthConfig {
                    max_auth_frame_bytes: 1,
                    ..WsAuthConfig::default()
                }
            ),
            Err(WsAuthError::FrameTooLarge { len: 2, max: 1 })
        );
        assert_eq!(
            parse_initial_auth_text("not-json", WsAuthConfig::default()),
            Err(WsAuthError::MalformedAuthFrame)
        );
        assert_eq!(
            parse_initial_auth_text(
                r#"{"messageType":"subscribe","token":"abc"}"#,
                WsAuthConfig::default()
            ),
            Err(WsAuthError::InvalidMessageType)
        );
        assert_eq!(
            parse_initial_auth_text(
                r#"{"messageType":"auth","token":" "}"#,
                WsAuthConfig::default()
            ),
            Err(WsAuthError::MissingToken)
        );
        assert_eq!(
            parse_initial_auth_text(
                r#"{"messageType":"auth","token":"abc\n"}"#,
                WsAuthConfig::default()
            ),
            Err(WsAuthError::MissingToken)
        );
        assert_eq!(
            reject_initial_auth_binary(),
            Err(WsAuthError::BinaryAuthFrame)
        );
    }

    #[test]
    fn auth_nonce_must_match() {
        let nonce = AuthNonce::new("nonce-1").unwrap();
        assert!(require_auth_nonce(&nonce, Some("nonce-1")).is_ok());
        assert_eq!(
            require_auth_nonce(&nonce, Some("nonce-2")),
            Err(WsAuthError::InvalidAuthNonce)
        );
        assert_eq!(
            require_auth_nonce(&nonce, Some(" nonce-1 ")),
            Err(WsAuthError::InvalidAuthNonce)
        );
        assert_eq!(AuthNonce::new(" "), Err(WsAuthError::InvalidAuthNonce));
        assert_eq!(
            AuthNonce::new(" nonce-1 "),
            Err(WsAuthError::InvalidAuthNonce)
        );
    }

    #[test]
    fn control_rate_limit_uses_sliding_window() {
        let start = Instant::now();
        let mut limit = ControlFrameRateLimit::new(2, Duration::from_secs(1));
        assert!(limit.allow_at(start));
        assert!(limit.allow_at(start + Duration::from_millis(900)));
        assert!(!limit.allow_at(start + Duration::from_millis(950)));
        assert!(limit.allow_at(start + Duration::from_secs(1)));
        assert!(!limit.allow_at(start + Duration::from_millis(1_001)));

        let mut zero_window = ControlFrameRateLimit::new(2, Duration::ZERO);
        assert!(!zero_window.allow_at(start));
    }
}
