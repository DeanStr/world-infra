//! Lightweight test helpers.
//!
//! Environment variables are process-global. Tests that mutate them should use
//! serial execution, unique names, or process isolation.

use std::{env, fmt, fs, io, path::Path, thread, time::Duration};

/// Restores an environment variable when dropped.
#[derive(Debug)]
pub struct EnvVarGuard {
    key: String,
    previous: Option<String>,
}

impl EnvVarGuard {
    /// Set `key` to `value` and restore the previous value on drop.
    #[must_use]
    pub fn set(key: impl Into<String>, value: impl Into<String>) -> Self {
        let key = key.into();
        let previous = env::var(&key).ok();
        env::set_var(&key, value.into());
        Self { key, previous }
    }

    /// Remove `key` and restore the previous value on drop.
    #[must_use]
    pub fn remove(key: impl Into<String>) -> Self {
        let key = key.into();
        let previous = env::var(&key).ok();
        env::remove_var(&key);
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            env::set_var(&self.key, previous);
        } else {
            env::remove_var(&self.key);
        }
    }
}

/// Error returned by eventual assertions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventuallyError {
    /// Number of attempts that were executed.
    pub attempts: usize,
    /// Last assertion error.
    pub last_error: String,
}

impl fmt::Display for EventuallyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "condition did not pass after {} attempts: {}",
            self.attempts, self.last_error
        )
    }
}

impl std::error::Error for EventuallyError {}

/// Retry a fallible assertion until it succeeds or attempts are exhausted.
///
/// # Errors
///
/// Returns the last assertion error when all attempts fail.
pub fn assert_eventually<E>(
    attempts: usize,
    delay: Duration,
    mut assertion: impl FnMut() -> Result<(), E>,
) -> Result<(), EventuallyError>
where
    E: fmt::Display,
{
    let attempts = attempts.max(1);
    let mut last_error = String::new();
    for attempt in 1..=attempts {
        match assertion() {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = error.to_string();
                if attempt < attempts {
                    thread::sleep(delay);
                }
            }
        }
    }
    Err(EventuallyError {
        attempts,
        last_error,
    })
}

/// Load a UTF-8 fixture with normalized line endings.
///
/// # Errors
///
/// Returns I/O errors from reading the fixture.
pub fn read_stable_fixture(path: impl AsRef<Path>) -> io::Result<String> {
    fs::read_to_string(path).map(|content| content.replace("\r\n", "\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_guard_restores_value() {
        let key = "WORLD_TEST_LITE_ENV_GUARD_RESTORES";
        env::set_var(key, "before");
        {
            let _guard = EnvVarGuard::set(key, "after");
            assert_eq!(env::var(key).unwrap(), "after");
        }
        assert_eq!(env::var(key).unwrap(), "before");
        env::remove_var(key);
    }

    #[test]
    fn eventual_assertion_retries() {
        let mut seen = 0;
        assert_eventually(3, Duration::from_millis(0), || {
            seen += 1;
            (seen == 2).then_some(()).ok_or("not yet")
        })
        .unwrap();
        assert_eq!(seen, 2);
    }
}
