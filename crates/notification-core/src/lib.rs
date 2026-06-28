//! Product-neutral notification delivery vocabulary.
//!
//! This crate intentionally does not define notification categories,
//! preferences, quiet hours, inbox semantics, templates, product SQL tables, or
//! provider clients. It provides shared delivery metadata that products can map
//! onto their own notification systems.

use std::{error::Error, fmt, str::FromStr, time::Duration};

use delivery_core::DeliveryAttemptOutcome;

const MAX_TARGET_LEN: usize = 1024;

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
    /// Returns [`NotificationError::InvalidDeliveryVersion`] for zero.
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

/// Metadata common to a claimed notification delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationDeliveryContext<Id> {
    /// Product-owned delivery id.
    pub delivery_id: Id,
    /// External delivery channel.
    pub channel: NotificationChannel,
    /// Version of the product-owned notification item.
    pub delivery_version: DeliveryVersion,
    /// Current attempt count.
    pub attempt: u32,
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
    fn maps_provider_outcomes_to_delivery_outcomes() {
        let outcome = NotificationProviderOutcome::RetryableFailure {
            retry_after: Some(Duration::from_secs(30)),
        };
        assert_eq!(
            outcome.as_delivery_outcome(),
            DeliveryAttemptOutcome::RetryableFailure {
                retry_after: Some(Duration::from_secs(30))
            }
        );
        assert_eq!(
            NotificationProviderOutcome::AmbiguousAfterSideEffect.as_delivery_outcome(),
            DeliveryAttemptOutcome::AmbiguousAfterSideEffect
        );
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
