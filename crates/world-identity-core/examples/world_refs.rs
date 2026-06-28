//! Explicit world reference examples.

use world_identity_core::{IncarnationId, NoIncarnation, WorldRef};

fn main() {
    let standalone_world = WorldRef::new("world-dev-001", NoIncarnation);
    let incarnated_world = WorldRef::new(42, IncarnationId("instance-001"));

    assert_eq!(
        standalone_world.no_incarnation_label(),
        "world:world-dev-001"
    );
    assert_eq!(
        incarnated_world.incarnation_label(),
        "world:42:incarnation:instance-001"
    );
}
