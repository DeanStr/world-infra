//! Basic environment parsing example.

use world_env::{parse_csv, parse_lenient_bool};

fn main() {
    let enabled = parse_lenient_bool("FEATURE_ENABLED", "yes").expect("valid bool");
    let headers = parse_csv("forwarded, x-forwarded-for, x-real-ip");
    assert!(enabled);
    assert_eq!(headers.len(), 3);
}
