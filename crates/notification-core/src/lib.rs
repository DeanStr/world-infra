//! Product-neutral notification delivery vocabulary.
//!
//! This crate intentionally does not define notification categories,
//! preferences, quiet hours, inbox semantics, templates, product SQL tables, or
//! provider clients. It provides shared delivery metadata that products can map
//! onto their own notification systems.

use std::{error::Error, fmt, num::NonZeroU32, str::FromStr, time::Duration};

use delivery_core::DeliveryAttemptOutcome;

const MAX_TARGET_LEN: usize = 1024;
const MAX_PROVIDER_FAILURE_CODE_LEN: usize = 128;

/// Error returned for malformed notification delivery metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotificationError {
    /// A string field was empty after trimming.
    Empty {
        /// Field name.
        field: &'static str,
    },
    /// A string field exceeded its maximum length.
    TooLong {
        /// Field name.
        field: &'static str,
        /// Observed length in bytes.
        len: usize,
        /// Maximum length in bytes.
        max: usize,
    },
    /// A field contained an unsupported control character.
    InvalidCharacter {
        /// Field name.
        field: &'static str,
        /// Invalid character.
        ch: char,
    },
    /// A channel label was unknown.
    UnknownChannel(String),
    /// Delivery version must be positive.
    InvalidDeliveryVersion,
    /// Delivery attempt must be positive.
    InvalidAttempt,
}

impl fmt::Display for NotificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { field } => write!(f, "{field} is empty"),
            Self::TooLong { field, len, max } => {
                write!(f, "{field} length {len} exceeds {max}")
            }
            Self::InvalidCharacter { field, ch } => {
                write!(f, "{field} contains invalid character {ch:?}")
            }
            Self::UnknownChannel(channel) => write!(f, "unknown notification channel {channel:?}"),
            Self::InvalidDeliveryVersion => f.write_str("delivery version must be positive"),
            Self::InvalidAttempt => f.write_str("delivery attempt must be positive"),
        }
    }
}

impl Error for NotificationError {}

/// Product-neutral external notification channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum NotificationChannel {
    /// SMTP or equivalent email delivery.
    Email,
    /// Browser Web Push delivery.
    BrowserPush,
    /// Firebase Cloud Messaging delivery.
    Fcm,
    /// Apple Push Notification service delivery.
    Apns,
    /// Product-owned webhook delivery.
    Webhook,
}

impl NotificationChannel {
    /// Stable lowercase channel label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::BrowserPush => "browser_push",
            Self::Fcm => "fcm",
            Self::Apns => "apns",
            Self::Webhook => "webhook",
        }
    }
}

impl fmt::Display for NotificationChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for NotificationChannel {
    type Err = NotificationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "email" | "smtp" => Ok(Self::Email),
            "browser_push" | "browser-push" | "web_push" | "web-push" | "push" => {
                Ok(Self::BrowserPush)
            }
            "fcm" => Ok(Self::Fcm),
            "apns" => Ok(Self::Apns),
            "webhook" => Ok(Self::Webhook),
            other => Err(NotificationError::UnknownChannel(other.to_owned())),
        }
    }
}

/// Positive version of a notification item being delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeliveryVersion(i32);

impl DeliveryVersion {
    /// Construct a delivery version.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::InvalidDeliveryVersion`] for zero or a
    /// negative value.
    pub const fn new(value: i32) -> Result<Self, NotificationError> {
        if value <= 0 {
            return Err(NotificationError::InvalidDeliveryVersion);
        }
        Ok(Self(value))
    }

    /// Construct a delivery version from a product SQL integer.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::InvalidDeliveryVersion`] for zero or a
    /// negative value.
    pub fn from_i32(value: i32) -> Result<Self, NotificationError> {
        Self::new(value)
    }

    /// Return the raw version.
    #[must_use]
    pub const fn get(self) -> i32 {
        self.0
    }

    /// Return the version as a SQL-friendly i32.
    #[must_use]
    pub const fn as_i32(self) -> i32 {
        self.0
    }
}

impl fmt::Display for DeliveryVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Product-neutral target address for an external notification.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NotificationTarget(String);

impl NotificationTarget {
    /// Validate and construct a target address.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError`] if the address is blank, too long, or
    /// contains control characters. Channel-specific validation remains product
    /// and provider policy.
    pub fn new(value: impl AsRef<str>) -> Result<Self, NotificationError> {
        let value = value.as_ref().trim();
        if value.is_empty() {
            return Err(NotificationError::Empty { field: "target" });
        }
        if value.len() > MAX_TARGET_LEN {
            return Err(NotificationError::TooLong {
                field: "target",
                len: value.len(),
                max: MAX_TARGET_LEN,
            });
        }
        if let Some(ch) = value.chars().find(|ch| ch.is_control()) {
            return Err(NotificationError::InvalidCharacter {
                field: "target",
                ch,
            });
        }
        Ok(Self(value.to_owned()))
    }

    /// Access the validated target string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NotificationTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for NotificationTarget {
    type Err = NotificationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

/// Positive 1-based provider attempt number for a notification claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NotificationAttempt(NonZeroU32);

impl NotificationAttempt {
    /// Construct a provider attempt number.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::InvalidAttempt`] for zero.
    pub const fn new(value: u32) -> Result<Self, NotificationError> {
        match NonZeroU32::new(value) {
            Some(value) => Ok(Self(value)),
            None => Err(NotificationError::InvalidAttempt),
        }
    }

    /// Return the raw attempt number.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }

    /// Return the attempt number as a SQL-friendly u32.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0.get()
    }
}

impl From<NonZeroU32> for NotificationAttempt {
    fn from(value: NonZeroU32) -> Self {
        Self(value)
    }
}

impl fmt::Display for NotificationAttempt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Metadata common to a claimed notification delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationDeliveryContext<Id> {
    /// Product-owned delivery id.
    pub delivery_id: Id,
    /// External delivery channel.
    pub channel: NotificationChannel,
    /// Version of the product-owned notification item.
    pub delivery_version: DeliveryVersion,
    /// Current 1-based provider attempt number for this claim.
    pub attempt: NotificationAttempt,
}

/// Sanitized, product-provided provider failure code.
///
/// This is intended for stable labels such as `smtp_response`,
/// `web_push_subscription_gone`, or `http_503`. It intentionally accepts only
/// lowercase ASCII letters, digits, and underscores so raw URLs, recipient
/// addresses, endpoints, bearer material, provider bodies, and diagnostics are
/// not accidentally normalized into shared metadata. Products should log
/// redacted provider details locally instead of storing them here.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderFailureCode(String);

impl ProviderFailureCode {
    /// Validate and construct a sanitized provider failure code.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError`] if the code is blank, too long, or
    /// contains unsupported characters.
    pub fn new(value: impl AsRef<str>) -> Result<Self, NotificationError> {
        let value = value.as_ref().trim();
        if value.is_empty() {
            return Err(NotificationError::Empty {
                field: "provider_failure_code",
            });
        }
        if value.len() > MAX_PROVIDER_FAILURE_CODE_LEN {
            return Err(NotificationError::TooLong {
                field: "provider_failure_code",
                len: value.len(),
                max: MAX_PROVIDER_FAILURE_CODE_LEN,
            });
        }
        if let Some(ch) = value
            .chars()
            .find(|ch| !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || *ch == '_'))
        {
            return Err(NotificationError::InvalidCharacter {
                field: "provider_failure_code",
                ch,
            });
        }
        Ok(Self(value.to_owned()))
    }

    /// Access the sanitized code.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProviderFailureCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for ProviderFailureCode {
    type Err = NotificationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

/// Failure-only provider classification.
///
/// This type deliberately has no `Accepted` variant. Successful attempts should
/// use [`NotificationProviderOutcome::Accepted`]. Products remain responsible
/// for deciding what a provider-specific failure means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ProviderFailureClass {
    /// The provider failure can be retried.
    Retryable,
    /// The provider rejected the target or message permanently.
    Permanent,
    /// The provider side effect may have happened, but completion is unknown.
    AmbiguousAfterSideEffect,
}

/// Sanitized metadata for a failed provider attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderFailure {
    class: ProviderFailureClass,
    retry_after: Option<Duration>,
    code: Option<ProviderFailureCode>,
}

impl ProviderFailure {
    /// Construct a retryable provider failure without a provider retry hint.
    #[must_use]
    pub const fn retryable() -> Self {
        Self {
            class: ProviderFailureClass::Retryable,
            retry_after: None,
            code: None,
        }
    }

    /// Construct a retryable provider failure with a provider retry hint.
    #[must_use]
    pub const fn retryable_after(retry_after: Duration) -> Self {
        Self {
            class: ProviderFailureClass::Retryable,
            retry_after: Some(retry_after),
            code: None,
        }
    }

    /// Construct a provider-permanent failure.
    #[must_use]
    pub const fn permanent() -> Self {
        Self {
            class: ProviderFailureClass::Permanent,
            retry_after: None,
            code: None,
        }
    }

    /// Construct an ambiguous-after-side-effect provider failure.
    #[must_use]
    pub const fn ambiguous_after_side_effect() -> Self {
        Self {
            class: ProviderFailureClass::AmbiguousAfterSideEffect,
            retry_after: None,
            code: None,
        }
    }

    /// Add a sanitized provider failure code.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError`] if the code is not a stable sanitized
    /// label. Do not pass raw provider bodies, endpoints, recipient addresses,
    /// tokens, or diagnostics.
    pub fn with_code(mut self, code: impl AsRef<str>) -> Result<Self, NotificationError> {
        self.code = Some(ProviderFailureCode::new(code)?);
        Ok(self)
    }

    /// Return the failure class.
    #[must_use]
    pub const fn class(&self) -> ProviderFailureClass {
        self.class
    }

    /// Return the optional provider retry hint.
    #[must_use]
    pub const fn retry_after(&self) -> Option<Duration> {
        self.retry_after
    }

    /// Return the optional sanitized provider failure code.
    #[must_use]
    pub fn code(&self) -> Option<&ProviderFailureCode> {
        self.code.as_ref()
    }

    /// Convert to provider outcome vocabulary.
    #[must_use]
    pub const fn as_provider_outcome(&self) -> NotificationProviderOutcome {
        match self.class {
            ProviderFailureClass::Retryable => NotificationProviderOutcome::RetryableFailure {
                retry_after: self.retry_after,
            },
            ProviderFailureClass::Permanent => NotificationProviderOutcome::PermanentFailure,
            ProviderFailureClass::AmbiguousAfterSideEffect => {
                NotificationProviderOutcome::AmbiguousAfterSideEffect
            }
        }
    }
}

impl From<ProviderFailure> for NotificationProviderOutcome {
    fn from(failure: ProviderFailure) -> Self {
        failure.as_provider_outcome()
    }
}

/// Provider attempt outcome before product persistence finalization.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotificationProviderOutcome {
    /// Provider accepted the notification.
    Accepted,
    /// Provider failed in a retryable way.
    RetryableFailure {
        /// Optional provider retry hint.
        retry_after: Option<Duration>,
    },
    /// Provider rejected the notification permanently.
    PermanentFailure,
    /// Provider side effect may have happened, but local completion is not known.
    AmbiguousAfterSideEffect,
}

impl NotificationProviderOutcome {
    /// Provider accepted the notification.
    #[must_use]
    pub const fn accepted() -> Self {
        Self::Accepted
    }

    /// Provider failed in a retryable way.
    #[must_use]
    pub const fn retryable(retry_after: Option<Duration>) -> Self {
        Self::RetryableFailure { retry_after }
    }

    /// Provider failed in a retryable way with a provider retry hint.
    #[must_use]
    pub const fn retryable_after(retry_after: Duration) -> Self {
        Self::RetryableFailure {
            retry_after: Some(retry_after),
        }
    }

    /// Provider failed in a retryable way without a provider retry hint.
    #[must_use]
    pub const fn retryable_without_hint() -> Self {
        Self::RetryableFailure { retry_after: None }
    }

    /// Provider rejected the notification permanently.
    #[must_use]
    pub const fn permanent_failure() -> Self {
        Self::PermanentFailure
    }

    /// Provider side effect may have happened, but local completion is unknown.
    #[must_use]
    pub const fn ambiguous_after_side_effect() -> Self {
        Self::AmbiguousAfterSideEffect
    }

    /// Convert sanitized failure metadata to provider outcome vocabulary.
    #[must_use]
    pub const fn from_provider_failure(failure: &ProviderFailure) -> Self {
        failure.as_provider_outcome()
    }

    /// Whether this outcome represents a retryable provider failure.
    #[must_use]
    pub const fn is_retryable_failure(&self) -> bool {
        matches!(self, Self::RetryableFailure { .. })
    }

    /// Whether this outcome represents a permanent provider failure.
    #[must_use]
    pub const fn is_permanent_failure(&self) -> bool {
        matches!(self, Self::PermanentFailure)
    }

    /// Whether this outcome represents an ambiguous provider side effect.
    #[must_use]
    pub const fn is_ambiguous_after_side_effect(&self) -> bool {
        matches!(self, Self::AmbiguousAfterSideEffect)
    }

    /// Convert to durable delivery-core outcome vocabulary.
    #[must_use]
    pub const fn as_delivery_outcome(&self) -> DeliveryAttemptOutcome {
        match self {
            Self::Accepted => DeliveryAttemptOutcome::Delivered,
            Self::RetryableFailure { retry_after } => DeliveryAttemptOutcome::RetryableFailure {
                retry_after: *retry_after,
            },
            Self::PermanentFailure => DeliveryAttemptOutcome::PermanentFailure,
            Self::AmbiguousAfterSideEffect => DeliveryAttemptOutcome::AmbiguousAfterSideEffect,
        }
    }
}

impl From<NotificationProviderOutcome> for DeliveryAttemptOutcome {
    fn from(outcome: NotificationProviderOutcome) -> Self {
        outcome.as_delivery_outcome()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_channel_aliases() {
        assert_eq!("smtp".parse(), Ok(NotificationChannel::Email));
        assert_eq!("web-push".parse(), Ok(NotificationChannel::BrowserPush));
        assert_eq!(NotificationChannel::Fcm.as_str(), "fcm");
    }

    #[test]
    fn validates_delivery_version() {
        assert_eq!(DeliveryVersion::from_i32(2).unwrap().as_i32(), 2);
        assert_eq!(
            DeliveryVersion::from_i32(0),
            Err(NotificationError::InvalidDeliveryVersion)
        );
        assert_eq!(
            DeliveryVersion::from_i32(-1),
            Err(NotificationError::InvalidDeliveryVersion)
        );
    }

    #[test]
    fn validates_notification_attempt() {
        assert_eq!(NotificationAttempt::new(2).unwrap().as_u32(), 2);
        assert_eq!(
            NotificationAttempt::new(0),
            Err(NotificationError::InvalidAttempt)
        );
    }

    #[test]
    fn maps_provider_outcomes_to_delivery_outcomes() {
        let outcome = NotificationProviderOutcome::retryable_after(Duration::from_secs(30));
        assert_eq!(
            outcome.as_delivery_outcome(),
            DeliveryAttemptOutcome::RetryableFailure {
                retry_after: Some(Duration::from_secs(30))
            }
        );
        assert_eq!(
            NotificationProviderOutcome::ambiguous_after_side_effect().as_delivery_outcome(),
            DeliveryAttemptOutcome::AmbiguousAfterSideEffect
        );
    }

    #[test]
    fn provider_failure_metadata_maps_to_existing_outcome() {
        let failure = ProviderFailure::retryable_after(Duration::from_secs(45))
            .with_code("http_503")
            .unwrap();
        assert_eq!(failure.class(), ProviderFailureClass::Retryable);
        assert_eq!(failure.code().unwrap().as_str(), "http_503");
        assert_eq!(
            failure.as_provider_outcome(),
            NotificationProviderOutcome::retryable_after(Duration::from_secs(45))
        );
        assert!(failure.as_provider_outcome().is_retryable_failure());
    }

    #[test]
    fn provider_failure_code_rejects_raw_provider_details() {
        assert!(ProviderFailureCode::new("web_push_subscription_gone").is_ok());
        assert!(matches!(
            ProviderFailureCode::new("https://push.example/token"),
            Err(NotificationError::InvalidCharacter { ch: ':', .. })
        ));
        assert!(matches!(
            ProviderFailureCode::new("user@example.test"),
            Err(NotificationError::InvalidCharacter { ch: '@', .. })
        ));
        assert!(matches!(
            ProviderFailureCode::new("smtp response"),
            Err(NotificationError::InvalidCharacter { ch: ' ', .. })
        ));
        assert!(matches!(
            ProviderFailureCode::new("bearer:secret"),
            Err(NotificationError::InvalidCharacter { ch: ':', .. })
        ));
        assert!(matches!(
            ProviderFailureCode::new("jwt.header.payload"),
            Err(NotificationError::InvalidCharacter { ch: '.', .. })
        ));
        assert!(matches!(
            ProviderFailureCode::new("HTTP_503"),
            Err(NotificationError::InvalidCharacter { ch: 'H', .. })
        ));
    }

    #[test]
    fn validates_targets_without_channel_policy() {
        assert_eq!(
            NotificationTarget::new(" user@example.test ")
                .unwrap()
                .as_str(),
            "user@example.test"
        );
        assert!(matches!(
            NotificationTarget::new("bad\nvalue"),
            Err(NotificationError::InvalidCharacter { .. })
        ));
    }
}
