//! Incarnation-aware world identity primitives.

use std::fmt;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Explicit marker for products that do not use incarnation keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NoIncarnation;

impl fmt::Display for NoIncarnation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("none")
    }
}

/// Required incarnation identifier wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IncarnationId<I>(pub I);

impl<I: fmt::Display> fmt::Display for IncarnationId<I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Explicit world reference. `I` should be [`NoIncarnation`] or a required
/// incarnation wrapper such as [`IncarnationId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct WorldRef<W, I> {
    /// Product-owned world identifier.
    pub world_id: W,
    /// Product-owned incarnation identifier or [`NoIncarnation`].
    pub incarnation: I,
}

impl<W, I> WorldRef<W, I> {
    /// Construct a world reference from explicit parts.
    #[must_use]
    pub const fn new(world_id: W, incarnation: I) -> Self {
        Self {
            world_id,
            incarnation,
        }
    }

    /// Borrow both identity dimensions without lossy conversion.
    #[must_use]
    pub const fn as_parts(&self) -> (&W, &I) {
        (&self.world_id, &self.incarnation)
    }
}

impl<W: fmt::Display> WorldRef<W, NoIncarnation> {
    /// Stable label for no-incarnation products. Use only for product-owned
    /// adapters, not as a universal wire format.
    #[must_use]
    pub fn no_incarnation_label(&self) -> String {
        format!("world:{}", self.world_id)
    }
}

impl<W: fmt::Display, I: fmt::Display> WorldRef<W, IncarnationId<I>> {
    /// Stable label for incarnation-aware products. Use only for product-owned
    /// adapters, not as a universal wire format.
    #[must_use]
    pub fn incarnation_label(&self) -> String {
        format!("world:{}:incarnation:{}", self.world_id, self.incarnation)
    }
}

/// Product-neutral fixture shape for explicit no-incarnation wire tests.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NoIncarnationWorldWire {
    /// World identifier encoded by the product adapter.
    pub world_id: String,
    /// Explicit no-incarnation marker.
    pub incarnation: String,
}

impl NoIncarnationWorldWire {
    /// Convert from a no-incarnation world reference.
    #[must_use]
    pub fn from_world_ref<W: fmt::Display>(world_ref: &WorldRef<W, NoIncarnation>) -> Self {
        Self {
            world_id: world_ref.world_id.to_string(),
            incarnation: "none".to_owned(),
        }
    }
}

/// Product-neutral fixture shape for required-incarnation wire tests.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IncarnatedWorldWire {
    /// World identifier encoded by the product adapter.
    pub world_id: String,
    /// Required incarnation identifier encoded by the product adapter.
    pub world_instance_id: String,
}

impl IncarnatedWorldWire {
    /// Convert from an incarnation-aware world reference.
    #[must_use]
    pub fn from_world_ref<W, I>(world_ref: &WorldRef<W, IncarnationId<I>>) -> Self
    where
        W: fmt::Display,
        I: fmt::Display,
    {
        Self {
            world_id: world_ref.world_id.to_string(),
            world_instance_id: world_ref.incarnation.0.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_incarnation_is_explicit() {
        let world = WorldRef::new("chairman-world", NoIncarnation);
        assert_eq!(world.no_incarnation_label(), "world:chairman-world");
        assert_eq!(
            NoIncarnationWorldWire::from_world_ref(&world),
            NoIncarnationWorldWire {
                world_id: "chairman-world".to_owned(),
                incarnation: "none".to_owned()
            }
        );
    }

    #[test]
    fn incarnation_is_required_in_wire_fixture() {
        let world = WorldRef::new(42, IncarnationId("instance-1"));
        assert_eq!(world.incarnation_label(), "world:42:incarnation:instance-1");
        assert_eq!(
            IncarnatedWorldWire::from_world_ref(&world),
            IncarnatedWorldWire {
                world_id: "42".to_owned(),
                world_instance_id: "instance-1".to_owned()
            }
        );
    }
}
