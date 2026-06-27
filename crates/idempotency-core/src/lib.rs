//! Idempotency key construction for new durable keys.
//!
//! This crate intentionally does not parse, normalize, or migrate existing
//! persisted product keys.

use std::{error::Error, fmt};

use world_identity_core::{IncarnationId, NoIncarnation, WorldRef};

/// Maximum length of a single key segment.
pub const MAX_SEGMENT_LEN: usize = 128;

/// Error returned for malformed idempotency key segments or keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    /// A segment was empty after trimming.
    EmptySegment,
    /// A segment exceeded [`MAX_SEGMENT_LEN`].
    SegmentTooLong {
        /// Observed segment length.
        len: usize,
    },
    /// A segment contained an unsupported character.
    InvalidCharacter {
        /// Invalid character.
        ch: char,
    },
    /// The key has no segments.
    EmptyKey,
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySegment => f.write_str("idempotency key segment is empty"),
            Self::SegmentTooLong { len } => {
                write!(
                    f,
                    "idempotency key segment length {len} exceeds {MAX_SEGMENT_LEN}"
                )
            }
            Self::InvalidCharacter { ch } => {
                write!(
                    f,
                    "idempotency key segment contains invalid character {ch:?}"
                )
            }
            Self::EmptyKey => f.write_str("idempotency key requires at least one segment"),
        }
    }
}

impl Error for KeyError {}

/// A validated key segment.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeySegment(String);

impl KeySegment {
    /// Validate a key segment.
    ///
    /// # Errors
    ///
    /// Returns [`KeyError`] if the segment is empty, too long, or contains
    /// unsupported characters.
    pub fn new(value: impl AsRef<str>) -> Result<Self, KeyError> {
        let value = value.as_ref().trim();
        if value.is_empty() {
            return Err(KeyError::EmptySegment);
        }
        if value.len() > MAX_SEGMENT_LEN {
            return Err(KeyError::SegmentTooLong { len: value.len() });
        }
        if let Some(ch) = value
            .chars()
            .find(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')))
        {
            return Err(KeyError::InvalidCharacter { ch });
        }
        Ok(Self(value.to_owned()))
    }

    /// Access the segment as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for KeySegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A validated idempotency key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    /// Build a key from validated segments.
    ///
    /// # Errors
    ///
    /// Returns [`KeyError::EmptyKey`] when no segments are supplied.
    pub fn from_segments(segments: impl IntoIterator<Item = KeySegment>) -> Result<Self, KeyError> {
        let values: Vec<String> = segments.into_iter().map(|segment| segment.0).collect();
        if values.is_empty() {
            return Err(KeyError::EmptyKey);
        }
        Ok(Self(values.join(":")))
    }

    /// Validate a fully formatted key.
    ///
    /// # Errors
    ///
    /// Returns [`KeyError`] if any segment is invalid.
    pub fn parse_new_key(value: impl AsRef<str>) -> Result<Self, KeyError> {
        let segments = value
            .as_ref()
            .split(':')
            .map(KeySegment::new)
            .collect::<Result<Vec<_>, _>>()?;
        Self::from_segments(segments)
    }

    /// Access the formatted key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for IdempotencyKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Builder for new idempotency keys.
#[derive(Debug, Clone, Default)]
pub struct KeyBuilder {
    segments: Vec<KeySegment>,
}

impl KeyBuilder {
    /// Create an empty builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a validated segment.
    ///
    /// # Errors
    ///
    /// Returns [`KeyError`] if the segment is invalid.
    pub fn push(mut self, segment: impl AsRef<str>) -> Result<Self, KeyError> {
        self.segments.push(KeySegment::new(segment)?);
        Ok(self)
    }

    /// Add all segments needed for a no-incarnation world reference.
    ///
    /// # Errors
    ///
    /// Returns [`KeyError`] if a formatted world segment is invalid.
    pub fn world<W: fmt::Display>(
        self,
        world: &WorldRef<W, NoIncarnation>,
    ) -> Result<Self, KeyError> {
        self.push("world")?.push(world.world_id.to_string())
    }

    /// Add all segments needed for an incarnation-aware world reference.
    ///
    /// # Errors
    ///
    /// Returns [`KeyError`] if a formatted world segment is invalid.
    pub fn incarnated_world<W, I>(
        self,
        world: &WorldRef<W, IncarnationId<I>>,
    ) -> Result<Self, KeyError>
    where
        W: fmt::Display,
        I: fmt::Display,
    {
        self.push("world")?
            .push(world.world_id.to_string())?
            .push("incarnation")?
            .push(world.incarnation.0.to_string())
    }

    /// Build the key.
    ///
    /// # Errors
    ///
    /// Returns [`KeyError::EmptyKey`] if no segments were supplied.
    pub fn build(self) -> Result<IdempotencyKey, KeyError> {
        IdempotencyKey::from_segments(self.segments)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_malformed_segments() {
        assert_eq!(KeySegment::new(""), Err(KeyError::EmptySegment));
        assert_eq!(
            KeySegment::new("bad/value"),
            Err(KeyError::InvalidCharacter { ch: '/' })
        );
    }

    #[test]
    fn builds_stable_world_key() {
        let world = WorldRef::new("world-dev-001", NoIncarnation);
        let key = KeyBuilder::new()
            .world(&world)
            .unwrap()
            .push("external-alert")
            .unwrap()
            .push("welcome")
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(key.as_str(), "world:world-dev-001:external-alert:welcome");
    }

    #[test]
    fn builds_stable_incarnated_key() {
        let world = WorldRef::new(7, IncarnationId("instance-a"));
        let key = KeyBuilder::new()
            .incarnated_world(&world)
            .unwrap()
            .push("cycle")
            .unwrap()
            .push("42")
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(key.as_str(), "world:7:incarnation:instance-a:cycle:42");
    }
}
