//! Product-neutral event fanout classification and transport vocabulary.
//!
//! This crate intentionally does not define product event schemas, durable
//! outbox tables, replay queries, authorization policy, or WebSocket wire
//! messages. Products remain authoritative for persisted events and use this
//! crate to share small fanout concepts: validated topics, source identity,
//! local publish outcomes, and ambiguous-after-attempt classification.

use std::{error::Error, fmt, num::NonZeroUsize, str::FromStr};

const MAX_TOPIC_LEN: usize = 256;
const MAX_NODE_ID_LEN: usize = 128;
const MAX_STABLE_ID_LEN: usize = 256;
const MAX_FAILURE_MESSAGE_LEN: usize = 1024;

/// Error returned for malformed fanout metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FanoutMetadataError {
    /// A metadata field was empty after trimming.
    Empty {
        /// Metadata field name.
        field: &'static str,
    },
    /// A metadata field exceeded its maximum length.
    TooLong {
        /// Metadata field name.
        field: &'static str,
        /// Observed length in bytes.
        len: usize,
        /// Maximum allowed length in bytes.
        max: usize,
    },
    /// A metadata field contained an unsupported character.
    InvalidCharacter {
        /// Metadata field name.
        field: &'static str,
        /// Invalid character.
        ch: char,
    },
}

impl fmt::Display for FanoutMetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { field } => write!(f, "{field} is empty"),
            Self::TooLong { field, len, max } => {
                write!(f, "{field} length {len} exceeds {max}")
            }
            Self::InvalidCharacter { field, ch } => {
                write!(f, "{field} contains invalid character {ch:?}")
            }
        }
    }
}

impl Error for FanoutMetadataError {}

fn validated_key_part(
    field: &'static str,
    value: impl AsRef<str>,
    max: usize,
) -> Result<String, FanoutMetadataError> {
    let value = value.as_ref().trim();
    if value.is_empty() {
        return Err(FanoutMetadataError::Empty { field });
    }
    if value.len() > max {
        return Err(FanoutMetadataError::TooLong {
            field,
            len: value.len(),
            max,
        });
    }
    if let Some(ch) = value
        .chars()
        .find(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':' | '/')))
    {
        return Err(FanoutMetadataError::InvalidCharacter { field, ch });
    }
    Ok(value.to_owned())
}

macro_rules! fanout_string_type {
    ($name:ident, $field:literal, $max:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Validate and construct this fanout metadata value.
            ///
            /// # Errors
            ///
            /// Returns [`FanoutMetadataError`] when the value is empty, too
            /// long, or contains unsupported characters.
            pub fn new(value: impl AsRef<str>) -> Result<Self, FanoutMetadataError> {
                validated_key_part($field, value, $max).map(Self)
            }

            /// Access the validated string.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = FanoutMetadataError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }
    };
}

fanout_string_type!(
    FanoutTopic,
    "topic",
    MAX_TOPIC_LEN,
    "A validated product-owned fanout topic or channel name."
);
fanout_string_type!(
    FanoutNodeId,
    "node_id",
    MAX_NODE_ID_LEN,
    "A validated product-owned process or node identifier."
);
fanout_string_type!(
    FanoutStableId,
    "stable_id",
    MAX_STABLE_ID_LEN,
    "A stable product-owned identifier for fanout dedupe and diagnostics."
);

/// Product-neutral source identity for a fanout message.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FanoutSource {
    /// Message originated in this process and has no node identifier.
    Local,
    /// Message originated from this product node.
    SelfOriginated {
        /// Product-owned node identifier.
        node_id: FanoutNodeId,
    },
    /// Message was received from a remote product node.
    Remote {
        /// Product-owned remote node identifier.
        node_id: FanoutNodeId,
    },
    /// Message was replayed from the product-owned durable event store.
    Replay,
}

impl FanoutSource {
    /// Return true when this source came from a product replay path.
    #[must_use]
    pub const fn is_replay(&self) -> bool {
        matches!(self, Self::Replay)
    }
}

/// Product-neutral envelope view for fanout adapters.
pub trait FanoutEnvelope {
    /// Topic or channel used to route the message.
    fn topic(&self) -> &FanoutTopic;

    /// Stable product-owned identifier for dedupe and diagnostics.
    fn stable_id(&self) -> Option<&FanoutStableId>;

    /// Source identity for the message.
    fn source(&self) -> &FanoutSource;
}

/// Failure class for a fanout attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FanoutFailureKind {
    /// Failure happened before the backend accepted or could have observed the
    /// publish.
    ClearFailure,
    /// Failure happened after the publish side effect may have reached the
    /// backend.
    AmbiguousAfterAttempt,
}

impl FanoutFailureKind {
    /// Return true when products should preserve an ambiguity boundary instead
    /// of treating the attempt as definitely failed.
    #[must_use]
    pub const fn requires_boundary_ambiguity(self) -> bool {
        matches!(self, Self::AmbiguousAfterAttempt)
    }
}

/// String-backed fanout failure detail for adapters that do not expose their
/// concrete error type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FanoutFailure {
    kind: FanoutFailureKind,
    message: String,
}

impl FanoutFailure {
    /// Construct a clear fanout failure.
    #[must_use]
    pub fn clear(message: impl fmt::Display) -> Self {
        Self::new(FanoutFailureKind::ClearFailure, message)
    }

    /// Construct an ambiguous-after-attempt fanout failure.
    #[must_use]
    pub fn ambiguous_after_attempt(message: impl fmt::Display) -> Self {
        Self::new(FanoutFailureKind::AmbiguousAfterAttempt, message)
    }

    /// Construct a fanout failure with an explicit kind.
    #[must_use]
    pub fn new(kind: FanoutFailureKind, message: impl fmt::Display) -> Self {
        let mut message = message.to_string();
        if message.len() > MAX_FAILURE_MESSAGE_LEN {
            let truncate_at = message
                .char_indices()
                .map(|(index, _)| index)
                .take_while(|index| *index <= MAX_FAILURE_MESSAGE_LEN)
                .last()
                .unwrap_or(0);
            message.truncate(truncate_at);
        }
        Self { kind, message }
    }

    /// Failure kind.
    #[must_use]
    pub const fn kind(&self) -> FanoutFailureKind {
        self.kind
    }

    /// Failure message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Return true when products should preserve an ambiguity boundary.
    #[must_use]
    pub const fn requires_boundary_ambiguity(&self) -> bool {
        self.kind.requires_boundary_ambiguity()
    }
}

impl fmt::Display for FanoutFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            FanoutFailureKind::ClearFailure => {
                write!(
                    f,
                    "event fanout failed before delivery was acknowledged: {}",
                    self.message
                )
            }
            FanoutFailureKind::AmbiguousAfterAttempt => {
                write!(
                    f,
                    "event fanout may have reached the backend before failing: {}",
                    self.message
                )
            }
        }
    }
}

impl Error for FanoutFailure {}

/// Product-neutral result of a best-effort publish.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PublishOutcome {
    /// At least one local subscriber or backend accepted the publish.
    Published {
        /// Known local subscriber count, when available.
        subscribers: Option<NonZeroUsize>,
    },
    /// Publish completed, but no local subscribers were present.
    NoSubscribers,
    /// Publish may have happened, but local completion is ambiguous.
    AmbiguousAfterAttempt {
        /// Failure detail.
        failure: FanoutFailure,
    },
}

impl PublishOutcome {
    /// Construct a published outcome when no subscriber count is known.
    #[must_use]
    pub const fn published() -> Self {
        Self::Published { subscribers: None }
    }

    /// Construct a local-broadcast outcome from a subscriber count.
    #[must_use]
    pub fn published_to(subscribers: usize) -> Self {
        match NonZeroUsize::new(subscribers) {
            Some(subscribers) => Self::Published {
                subscribers: Some(subscribers),
            },
            None => Self::NoSubscribers,
        }
    }

    /// Construct an ambiguous-after-attempt outcome.
    #[must_use]
    pub fn ambiguous_after_attempt(message: impl fmt::Display) -> Self {
        Self::AmbiguousAfterAttempt {
            failure: FanoutFailure::ambiguous_after_attempt(message),
        }
    }

    /// Return true when the publish reached at least one known subscriber or
    /// backend.
    #[must_use]
    pub const fn is_published(&self) -> bool {
        matches!(self, Self::Published { .. })
    }

    /// Return true when the publish completed with no local subscribers.
    #[must_use]
    pub const fn is_no_subscribers(&self) -> bool {
        matches!(self, Self::NoSubscribers)
    }

    /// Return true when the publish may have happened and needs product-owned
    /// reconciliation.
    #[must_use]
    pub const fn requires_boundary_ambiguity(&self) -> bool {
        matches!(self, Self::AmbiguousAfterAttempt { .. })
    }
}

/// Classify a local broadcast send result whose `Err` means there were no
/// active subscribers.
#[must_use]
pub fn local_broadcast_outcome<E>(send_result: Result<usize, E>) -> PublishOutcome {
    match send_result {
        Ok(subscribers) => PublishOutcome::published_to(subscribers),
        Err(_error) => PublishOutcome::NoSubscribers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_validation_rejects_blank_long_and_unsafe_values() {
        assert_eq!(
            FanoutTopic::new(" ").unwrap_err(),
            FanoutMetadataError::Empty { field: "topic" }
        );
        assert!(matches!(
            FanoutTopic::new("a".repeat(MAX_TOPIC_LEN + 1)).unwrap_err(),
            FanoutMetadataError::TooLong { field: "topic", .. }
        ));
        assert!(matches!(
            FanoutTopic::new("world events").unwrap_err(),
            FanoutMetadataError::InvalidCharacter {
                field: "topic",
                ch: ' '
            }
        ));
        assert_eq!(
            FanoutTopic::new("prod:world/2.cycles").unwrap().as_str(),
            "prod:world/2.cycles"
        );
    }

    #[test]
    fn failure_kind_preserves_boundary_ambiguity() {
        assert!(FanoutFailureKind::AmbiguousAfterAttempt.requires_boundary_ambiguity());
        assert!(!FanoutFailureKind::ClearFailure.requires_boundary_ambiguity());

        let failure = FanoutFailure::ambiguous_after_attempt("redis publish timeout");
        assert_eq!(failure.kind(), FanoutFailureKind::AmbiguousAfterAttempt);
        assert!(failure.requires_boundary_ambiguity());
    }

    #[test]
    fn local_broadcast_result_tracks_subscriber_count() {
        assert_eq!(
            local_broadcast_outcome::<()>(Ok(2)),
            PublishOutcome::Published {
                subscribers: NonZeroUsize::new(2)
            }
        );
        assert_eq!(
            local_broadcast_outcome::<()>(Ok(0)),
            PublishOutcome::NoSubscribers
        );
        assert_eq!(
            local_broadcast_outcome(Err(())),
            PublishOutcome::NoSubscribers
        );
    }

    #[test]
    fn failure_message_truncates_on_utf8_boundary() {
        let message = format!(
            "{}{}",
            "a".repeat(MAX_FAILURE_MESSAGE_LEN - 1),
            "é".repeat(8)
        );

        let failure = FanoutFailure::clear(message);

        assert_eq!(failure.message().len(), MAX_FAILURE_MESSAGE_LEN - 1);
        assert!(failure.message().is_char_boundary(failure.message().len()));
    }

    #[test]
    fn envelope_trait_keeps_payload_product_owned() {
        struct TestEnvelope {
            topic: FanoutTopic,
            stable_id: FanoutStableId,
            source: FanoutSource,
        }

        impl FanoutEnvelope for TestEnvelope {
            fn topic(&self) -> &FanoutTopic {
                &self.topic
            }

            fn stable_id(&self) -> Option<&FanoutStableId> {
                Some(&self.stable_id)
            }

            fn source(&self) -> &FanoutSource {
                &self.source
            }
        }

        let envelope = TestEnvelope {
            topic: FanoutTopic::new("world.1.cycles").unwrap(),
            stable_id: FanoutStableId::new("cycle-completed:1:42").unwrap(),
            source: FanoutSource::Replay,
        };

        assert_eq!(envelope.topic().as_str(), "world.1.cycles");
        assert_eq!(
            envelope.stable_id().map(FanoutStableId::as_str),
            Some("cycle-completed:1:42")
        );
        assert!(envelope.source().is_replay());
    }
}
