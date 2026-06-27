//! In-process fixed-window rate limit example.

use std::{
    num::NonZeroU32,
    time::{Duration, SystemTime},
};

use rate_limit_core::{InProcessFixedWindow, LimitSpec, Namespace, RateLimitKey};

fn main() {
    let backend = InProcessFixedWindow::new();
    let namespace = Namespace::new("example").expect("safe namespace");
    let key = RateLimitKey::new("anonymous").expect("safe key");
    let limit = LimitSpec::Fixed {
        count: NonZeroU32::new(1).expect("non-zero"),
        window: Duration::from_secs(60),
    };

    assert!(
        backend
            .check(&namespace, &key, limit, SystemTime::UNIX_EPOCH)
            .unwrap()
            .allowed
    );
    assert!(
        !backend
            .check(&namespace, &key, limit, SystemTime::UNIX_EPOCH)
            .unwrap()
            .allowed
    );
}
