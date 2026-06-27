//! Environment guard example.

use world_test_lite::EnvVarGuard;

fn main() {
    let _guard = EnvVarGuard::set("WORLD_TEST_LITE_EXAMPLE", "enabled");
    assert_eq!(
        std::env::var("WORLD_TEST_LITE_EXAMPLE").as_deref(),
        Ok("enabled")
    );
}
