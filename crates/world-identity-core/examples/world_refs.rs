//! Explicit world reference examples.

use world_identity_core::{IncarnationId, NoIncarnation, WorldRef};

fn main() {
    let chairman = WorldRef::new("world-dev-001", NoIncarnation);
    let airline = WorldRef::new(42, IncarnationId("instance-001"));

    assert_eq!(chairman.no_incarnation_label(), "world:world-dev-001");
    assert_eq!(
        airline.incarnation_label(),
        "world:42:incarnation:instance-001"
    );
}
