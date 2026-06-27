//! Reviewed `SET LOCAL` statement example.

use tenant_scope_sqlx::{set_local_statement, SettingName};

fn main() {
    let name = SettingName::new("chairman.world_id").expect("safe setting name");
    let statement = set_local_statement(&name, "world-dev-001").expect("safe value");
    assert_eq!(statement, "SET LOCAL chairman.world_id = 'world-dev-001'");
}
