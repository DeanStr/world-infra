//! Product-neutral event envelope metadata.
//!
//! This crate defines validated event metadata and a generic envelope type. It
//! intentionally does not define product event payloads, transport topics,
//! database tables, or a universal serialized wire shape.

use std::{error::Error, fmt, str::FromStr, time::SystemTime};

use idempotency_core::{IdempotencyKey, KeyError};
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
    /// A required builder field was not provided.
    MissingRequired {
        /// Metadata field name.
        field: &'static str,
    },
    /// Idempotency key validation failed.
    InvalidIdempotencyKey {
        /// Underlying idempotency key error.
        error: KeyError,
    },
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
            Self::MissingRequired { field } => write!(f, "{field} is required"),
            Self::InvalidIdempotencyKey { error } => {
                write!(f, "idempotency_key is invalid: {error}")
            }
        }
    }
}

impl Error for EventMetadataError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidIdempotencyKey { error } => Some(error),
            Self::Empty { .. }
            | Self::TooLong { .. }
            | Self::InvalidCharacter { .. }
            | Self::ZeroSchemaVersion
            | Self::MissingRequired { .. } => None,
        }
    }
}

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

/// Builder for validated event metadata.
///
/// The builder deliberately requires products to choose the durability boundary
/// and production timestamp. Use [`EventMetadataBuilder::produced_now`] only
/// when wall-clock time is the intended product behavior.
#[derive(Debug, Clone)]
pub struct EventMetadataBuilder<W, I> {
    event_id: EventId,
    event_type: EventType,
    schema_version: SchemaVersion,
    world: WorldRef<W, I>,
    aggregate_id: Option<AggregateId>,
    idempotency_key: Option<IdempotencyKey>,
    produced_at: Option<SystemTime>,
    source: EventSource,
    causation_id: Option<EventId>,
    correlation_id: Option<CorrelationId>,
    durability: Option<DurabilityBoundary>,
}

impl<W, I> EventMetadataBuilder<W, I> {
    /// Start building event metadata from required product-owned fields.
    ///
    /// # Errors
    ///
    /// Returns [`EventMetadataError`] when any required metadata value is
    /// invalid.
    pub fn new(
        world: WorldRef<W, I>,
        event_id: impl AsRef<str>,
        event_type: impl AsRef<str>,
        schema_version: u32,
        source: impl AsRef<str>,
    ) -> Result<Self, EventMetadataError> {
        Ok(Self {
            event_id: EventId::new(event_id)?,
            event_type: EventType::new(event_type)?,
            schema_version: SchemaVersion::new(schema_version)?,
            world,
            aggregate_id: None,
            idempotency_key: None,
            produced_at: None,
            source: EventSource::new(source)?,
            causation_id: None,
            correlation_id: None,
            durability: None,
        })
    }

    /// Set the product-owned aggregate identifier.
    ///
    /// # Errors
    ///
    /// Returns [`EventMetadataError`] when the aggregate identifier is invalid.
    pub fn aggregate_id(mut self, value: impl AsRef<str>) -> Result<Self, EventMetadataError> {
        self.aggregate_id = Some(AggregateId::new(value)?);
        Ok(self)
    }

    /// Set a pre-validated product-owned aggregate identifier.
    #[must_use]
    pub fn aggregate_id_value(mut self, value: AggregateId) -> Self {
        self.aggregate_id = Some(value);
        self
    }

    /// Set the durable idempotency key.
    ///
    /// # Errors
    ///
    /// Returns [`EventMetadataError`] when the idempotency key is invalid.
    pub fn idempotency_key(mut self, value: impl AsRef<str>) -> Result<Self, EventMetadataError> {
        self.idempotency_key = Some(
            IdempotencyKey::parse_new_key(value)
                .map_err(|error| EventMetadataError::InvalidIdempotencyKey { error })?,
        );
        Ok(self)
    }

    /// Set a pre-validated durable idempotency key.
    #[must_use]
    pub fn idempotency_key_value(mut self, value: IdempotencyKey) -> Self {
        self.idempotency_key = Some(value);
        self
    }

    /// Set the event that caused this event.
    ///
    /// # Errors
    ///
    /// Returns [`EventMetadataError`] when the causation identifier is invalid.
    pub fn causation_id(mut self, value: impl AsRef<str>) -> Result<Self, EventMetadataError> {
        self.causation_id = Some(EventId::new(value)?);
        Ok(self)
    }

    /// Set a pre-validated causation event identifier.
    #[must_use]
    pub fn causation_id_value(mut self, value: EventId) -> Self {
        self.causation_id = Some(value);
        self
    }

    /// Set the cross-boundary correlation identifier.
    ///
    /// # Errors
    ///
    /// Returns [`EventMetadataError`] when the correlation identifier is
    /// invalid.
    pub fn correlation_id(mut self, value: impl AsRef<str>) -> Result<Self, EventMetadataError> {
        self.correlation_id = Some(CorrelationId::new(value)?);
        Ok(self)
    }

    /// Set a pre-validated cross-boundary correlation identifier.
    #[must_use]
    pub fn correlation_id_value(mut self, value: CorrelationId) -> Self {
        self.correlation_id = Some(value);
        self
    }

    /// Set the product-owned production timestamp explicitly.
    #[must_use]
    pub fn produced_at(mut self, value: SystemTime) -> Self {
        self.produced_at = Some(value);
        self
    }

    /// Set the production timestamp to [`SystemTime::now`].
    #[must_use]
    pub fn produced_now(self) -> Self {
        self.produced_at(SystemTime::now())
    }

    /// Set the product-neutral durability boundary.
    #[must_use]
    pub fn durability(mut self, value: DurabilityBoundary) -> Self {
        self.durability = Some(value);
        self
    }

    /// Build the validated metadata.
    ///
    /// # Errors
    ///
    /// Returns [`EventMetadataError::MissingRequired`] if `produced_at` or
    /// `durability` was not set.
    pub fn build(self) -> Result<EventMetadata<W, I>, EventMetadataError> {
        let produced_at = self
            .produced_at
            .ok_or(EventMetadataError::MissingRequired {
                field: "produced_at",
            })?;
        let durability = self.durability.ok_or(EventMetadataError::MissingRequired {
            field: "durability",
        })?;
        Ok(EventMetadata {
            event_id: self.event_id,
            event_type: self.event_type,
            schema_version: self.schema_version,
            world: self.world,
            aggregate_id: self.aggregate_id,
            idempotency_key: self.idempotency_key,
            produced_at,
            source: self.source,
            causation_id: self.causation_id,
            correlation_id: self.correlation_id,
            durability,
        })
    }

    /// Build a validated envelope with product-owned payload.
    ///
    /// # Errors
    ///
    /// Returns [`EventMetadataError::MissingRequired`] if `produced_at` or
    /// `durability` was not set.
    pub fn build_envelope<P>(
        self,
        payload: P,
    ) -> Result<EventEnvelope<W, I, P>, EventMetadataError> {
        Ok(EventEnvelope::from_parts(self.build()?, payload))
    }
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

    #[test]
    fn builder_constructs_no_incarnation_metadata_and_envelope() {
        let world = WorldRef::new("chairman-world", NoIncarnation);
        let envelope = EventMetadataBuilder::new(
            world,
            "cycle-completed:chairman-world:7",
            "world.cycle_completed",
            1,
            "chairman-game-db",
        )
        .unwrap()
        .aggregate_id("world_cycle:job-7")
        .unwrap()
        .idempotency_key("world:chairman-world:cycle-completed:7")
        .unwrap()
        .produced_at(UNIX_EPOCH)
        .durability(DurabilityBoundary::DurableOutboxRecorded)
        .build_envelope("payload")
        .unwrap();

        assert_eq!(
            envelope.metadata.event_id.as_str(),
            "cycle-completed:chairman-world:7"
        );
        assert_eq!(envelope.metadata.schema_version.get(), 1);
        assert_eq!(
            envelope.metadata.world.no_incarnation_label(),
            "world:chairman-world"
        );
        assert_eq!(
            envelope
                .metadata
                .aggregate_id
                .as_ref()
                .map(AggregateId::as_str),
            Some("world_cycle:job-7")
        );
        assert_eq!(envelope.metadata.produced_at, UNIX_EPOCH);
        assert_eq!(
            envelope.metadata.durability,
            DurabilityBoundary::DurableOutboxRecorded
        );
        assert_eq!(envelope.payload, "payload");
    }

    #[test]
    fn builder_constructs_incarnation_aware_metadata() {
        let world = WorldRef::new(1, IncarnationId("instance-1"));
        let idempotency_key =
            IdempotencyKey::parse_new_key("cycle_event_broadcast:world.1.instance.instance-1.2")
                .unwrap();
        let metadata = EventMetadataBuilder::new(
            world,
            "cycle-completed:1:instance-1:2",
            "cycleCompleted",
            2,
            "loco-app",
        )
        .unwrap()
        .idempotency_key_value(idempotency_key.clone())
        .causation_id("finalization:1:instance-1:2")
        .unwrap()
        .correlation_id("world:1:instance:instance-1")
        .unwrap()
        .produced_at(UNIX_EPOCH)
        .durability(DurabilityBoundary::PublishAccepted)
        .build()
        .unwrap();

        assert_eq!(
            metadata.world.incarnation_label(),
            "world:1:incarnation:instance-1"
        );
        assert_eq!(metadata.idempotency_key, Some(idempotency_key));
        assert_eq!(
            metadata.causation_id.as_ref().map(EventId::as_str),
            Some("finalization:1:instance-1:2")
        );
        assert_eq!(
            metadata.correlation_id.as_ref().map(CorrelationId::as_str),
            Some("world:1:instance:instance-1")
        );
        assert_eq!(metadata.durability, DurabilityBoundary::PublishAccepted);
    }

    #[test]
    fn builder_rejects_missing_required_fields_and_invalid_keys() {
        let world = WorldRef::new("chairman-world", NoIncarnation);
        let builder =
            EventMetadataBuilder::new(world, "event-1", "world.cycle_completed", 1, "source")
                .unwrap();
        assert_eq!(
            builder.clone().build(),
            Err(EventMetadataError::MissingRequired {
                field: "produced_at"
            })
        );
        assert_eq!(
            builder.produced_at(UNIX_EPOCH).build(),
            Err(EventMetadataError::MissingRequired {
                field: "durability"
            })
        );

        let world = WorldRef::new("chairman-world", NoIncarnation);
        assert!(matches!(
            EventMetadataBuilder::new(world, "event-1", "world.cycle_completed", 1, "source")
                .unwrap()
                .idempotency_key("bad key"),
            Err(EventMetadataError::InvalidIdempotencyKey { .. })
        ));
    }
}
