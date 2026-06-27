//! New idempotency key construction example.

use idempotency_core::KeyBuilder;
use world_identity_core::{NoIncarnation, WorldRef};

fn main() {
    let world = WorldRef::new("world-dev-001", NoIncarnation);
    let key = KeyBuilder::new()
        .world(&world)
        .expect("world id is segment-safe")
        .push("delivery")
        .expect("literal segment is safe")
        .push("welcome")
        .expect("literal segment is safe")
        .build()
        .expect("segments exist");

    assert_eq!(key.as_str(), "world:world-dev-001:delivery:welcome");
}
