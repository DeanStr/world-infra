//! Client IP extraction example.

use http_primitives::{extract_client_ip, parse_trusted_proxies};

fn main() {
    let proxies = parse_trusted_proxies("10.0.0.0/8").expect("valid CIDR");
    let client = extract_client_ip(
        "10.1.1.1".parse().expect("valid peer"),
        &proxies,
        Some(r#"for="203.0.113.10";proto=https"#),
        None,
        None,
    );
    assert_eq!(client.to_string(), "203.0.113.10");
}
