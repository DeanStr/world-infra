//! Product-neutral event envelope metadata.
//!
//! This crate defines validated event metadata and a generic envelope type. It
//! intentionally does not define product event payloads, transport topics,
//! database tables, or a universal serialized wire shape.

use std::{error::Error, fmt, str::FromStr, time::SystemTime};

use idempotency_core::IdempotencyKey;
use world_identity_core::WorldRef;

const MAX_EVENT_ID_LEN: usize = 256;
const MAX_EVENT_TYPE_LEN: usize = 128;
const MAX_AGGREGATE_ID_LEN: usize = 256;
const MAX_EVENT_SOURCE_LEN: usize = 128;
const MAX_CORRELATION_ID_LEN: usize = 256;

/// Error returned for malformed event metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EventMetadataError {
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
    /// Schema version zero is invalid.
    ZeroSchemaVersion,
}

impl fmt::Display for EventMetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { field } => write!(f, "{field} is empty"),
            Self::TooLong { field, len, max } => {
                write!(f, "{field} length {len} exceeds {max}")
            }
            Self::InvalidCharacter { field, ch } => {
                write!(f, "{field} contains invalid character {ch:?}")
            }
            Self::ZeroSchemaVersion => f.write_str("schema version must be greater than zero"),
        }
    }
}

impl Error for EventMetadataError {}

fn validated_metadata(
    field: &'static str,
    value: impl AsRef<str>,
    max: usize,
    allow_colon: bool,
) -> Result<String, EventMetadataError> {
    let value = value.as_ref();
    if let Some(ch) = value.chars().find(|ch| ch.is_control()) {
        return Err(EventMetadataError::InvalidCharacter { field, ch });
    }
    let value = value.trim();
    if value.is_empty() {
        return Err(EventMetadataError::Empty { field });
    }
    if value.len() > max {
        return Err(EventMetadataError::TooLong {
            field,
            len: value.len(),
            max,
        });
    }
    if let Some(ch) = value.chars().find(|ch| {
        !(ch.is_ascii_alphanumeric()
            || matches!(ch, '-' | '_' | '.')
            || (allow_colon && *ch == ':'))
    }) {
        return Err(EventMetadataError::InvalidCharacter { field, ch });
    }
    Ok(value.to_owned())
}

macro_rules! metadata_string_type {
    ($name:ident, $field:literal, $max:ident, $allow_colon:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Validate and construct this metadata value.
            ///
            /// # Errors
            ///
            /// Returns [`EventMetadataError`] when the value is empty, too long,
            /// or contains unsupported characters.
            pub fn new(value: impl AsRef<str>) -> Result<Self, EventMetadataError> {
                validated_metadata($field, value, $max, $allow_colon).map(Self)
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
            type Err = EventMetadataError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }
    };
}

metadata_string_type!(
    EventId,
    "event_id",
    MAX_EVENT_ID_LEN,
    true,
    "A stable product-owned event identifier."
);
metadata_string_type!(
    EventType,
    "event_type",
    MAX_EVENT_TYPE_LEN,
    false,
    "A product-owned event type name."
);
metadata_string_type!(
    AggregateId,
    "aggregate_id",
    MAX_AGGREGATE_ID_LEN,
    true,
    "A product-owned aggregate identifier."
);
metadata_string_type!(
    EventSource,
    "source",
    MAX_EVENT_SOURCE_LEN,
    false,
    "A product-owned event source label."
);
metadata_string_type!(
    CorrelationId,
    "correlation_id",
    MAX_CORRELATION_ID_LEN,
    true,
    "A product-owned correlation identifier."
);

/// A positive event schema version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SchemaVersion(u32);

impl SchemaVersion {
    /// Validate and construct a schema version.
    ///
    /// # Errors
    ///
    /// Returns [`EventMetadataError::ZeroSchemaVersion`] for zero.
    pub const fn new(value: u32) -> Result<Self, EventMetadataError> {
        if value == 0 {
            return Err(EventMetadataError::ZeroSchemaVersion);
        }
        Ok(Self(value))
    }

    /// Return the raw version.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for SchemaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Product-neutral durability boundary vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DurabilityBoundary {
    /// Non-durable runtime signal.
    EphemeralSignal,
    /// Authoritative product state has committed.
    AuthoritativeStateCommitted,
    /// A durable product-owned outbox row has been recorded.
    DurableOutboxRecorded,
    /// Product-owned publish/fanout accepted the event.
    PublishAccepted,
    /// Publish/fanout may have happened, but local completion is ambiguous.
    PublishAttemptAmbiguous,
}

/// Product-neutral event metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventMetadata<W, I> {
    /// Stable product-owned event identifier.
    pub event_id: EventId,
    /// Product-owned event type.
    pub event_type: EventType,
    /// Positive schema version.
    pub schema_version: SchemaVersion,
    /// Shared world reference.
    pub world: WorldRef<W, I>,
    /// Optional product-owned aggregate identifier.
    pub aggregate_id: Option<AggregateId>,
    /// Optional idempotency key for durable side effects.
    pub idempotency_key: Option<IdempotencyKey>,
    /// Time the product produced the event.
    pub produced_at: SystemTime,
    /// Product-owned event source label.
    pub source: EventSource,
    /// Optional event that caused this event.
    pub causation_id: Option<EventId>,
    /// Optional cross-boundary correlation identifier.
    pub correlation_id: Option<CorrelationId>,
    /// Product-neutral durability boundary label.
    pub durability: DurabilityBoundary,
}

/// Generic event envelope with product-owned payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventEnvelope<W, I, P> {
    /// Product-neutral event metadata.
    pub metadata: EventMetadata<W, I>,
    /// Product-owned payload.
    pub payload: P,
}

impl<W, I, P> EventEnvelope<W, I, P> {
    /// Construct an envelope from metadata and payload.
    #[must_use]
    pub const fn from_parts(metadata: EventMetadata<W, I>, payload: P) -> Self {
        Self { metadata, payload }
    }

    /// Borrow the envelope metadata.
    #[must_use]
    pub const fn metadata(&self) -> &EventMetadata<W, I> {
        &self.metadata
    }

    /// Borrow the product-owned payload.
    #[must_use]
    pub const fn payload(&self) -> &P {
        &self.payload
    }

    /// Split the envelope into metadata and payload.
    #[must_use]
    pub fn into_parts(self) -> (EventMetadata<W, I>, P) {
        (self.metadata, self.payload)
    }
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use idempotency_core::IdempotencyKey;
    use world_identity_core::{IncarnationId, NoIncarnation};

    use super::*;

    #[test]
    fn validates_metadata_strings() {
        assert_eq!(
            EventType::new("world.cycle_completed ").unwrap().as_str(),
            "world.cycle_completed"
        );
        assert_eq!(
            "source.service".parse::<EventSource>().unwrap().as_str(),
            "source.service"
        );
        assert_eq!(
            CorrelationId::new("request:123").unwrap().to_string(),
            "request:123"
        );
        assert_eq!(
            EventId::new("cycle-completed:1:instance-1:2")
                .unwrap()
                .as_str(),
            "cycle-completed:1:instance-1:2"
        );
        assert_eq!(
            EventType::new("bad type"),
            Err(EventMetadataError::InvalidCharacter {
                field: "event_type",
                ch: ' '
            })
        );
        assert_eq!(
            EventType::new("bad:type"),
            Err(EventMetadataError::InvalidCharacter {
                field: "event_type",
                ch: ':'
            })
        );
        assert_eq!(
            EventType::new("world.cycle_completed\n"),
            Err(EventMetadataError::InvalidCharacter {
                field: "event_type",
                ch: '\n'
            })
        );
        assert_eq!(
            SchemaVersion::new(0),
            Err(EventMetadataError::ZeroSchemaVersion)
        );
        assert_eq!(SchemaVersion::new(2).unwrap().get(), 2);
        assert_eq!(SchemaVersion::new(2).unwrap().to_string(), "2");
        assert_eq!(
            EventMetadataError::TooLong {
                field: "event_type",
                len: 129,
                max: 128,
            }
            .to_string(),
            "event_type length 129 exceeds 128"
        );
        assert_eq!(
            EventMetadataError::ZeroSchemaVersion.to_string(),
            "schema version must be greater than zero"
        );
    }

    #[test]
    fn constructs_no_incarnation_envelope() {
        let world = WorldRef::new("chairman-world", NoIncarnation);
        let metadata = EventMetadata {
            event_id: EventId::new("11111111-1111-1111-1111-111111111111").unwrap(),
            event_type: EventType::new("world.cycle_completed").unwrap(),
            schema_version: SchemaVersion::new(1).unwrap(),
            world,
            aggregate_id: Some(AggregateId::new("world_cycle:job-1").unwrap()),
            idempotency_key: Some(
                IdempotencyKey::parse_new_key("world:chairman-world:cycle_completed").unwrap(),
            ),
            produced_at: UNIX_EPOCH,
            source: EventSource::new("chairman-game-db").unwrap(),
            causation_id: None,
            correlation_id: None,
            durability: DurabilityBoundary::DurableOutboxRecorded,
        };

        let envelope = EventEnvelope::from_parts(metadata, "payload-owned-by-product");
        assert_eq!(
            envelope.metadata().event_type.as_str(),
            "world.cycle_completed"
        );
        assert_eq!(envelope.payload(), &"payload-owned-by-product");
        assert_eq!(
            envelope.metadata.world.no_incarnation_label(),
            "world:chairman-world"
        );
        assert_eq!(envelope.payload, "payload-owned-by-product");
    }

    #[test]
    fn constructs_incarnation_aware_envelope() {
        let world = WorldRef::new(1, IncarnationId("instance-1"));
        let metadata = EventMetadata {
            event_id: EventId::new("cycle-completed:1:instance-1:2").unwrap(),
            event_type: EventType::new("cycleCompleted").unwrap(),
            schema_version: SchemaVersion::new(1).unwrap(),
            world,
            aggregate_id: Some(AggregateId::new("world:1:cycle:2").unwrap()),
            idempotency_key: Some(
                IdempotencyKey::parse_new_key("cycle_event_broadcast:world.1.cycle.2").unwrap(),
            ),
            produced_at: UNIX_EPOCH,
            source: EventSource::new("loco-app").unwrap(),
            causation_id: Some(EventId::new("finalization:1:instance-1:2").unwrap()),
            correlation_id: Some(CorrelationId::new("world:1:instance:instance-1").unwrap()),
            durability: DurabilityBoundary::PublishAccepted,
        };

        let envelope = EventEnvelope::from_parts(metadata, "payload");
        let (metadata, payload) = envelope.into_parts();
        assert_eq!(
            metadata.world.incarnation_label(),
            "world:1:incarnation:instance-1"
        );
        assert_eq!(payload, "payload");
    }
}
