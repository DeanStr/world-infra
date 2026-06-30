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

/// Collection of environment guards restored together on drop.
#[derive(Debug)]
pub struct EnvVarGuards {
    guards: Vec<EnvVarGuard>,
}

impl EnvVarGuards {
    /// Return the number of guarded variables.
    #[must_use]
    pub fn len(&self) -> usize {
        self.guards.len()
    }

    /// Return whether no variables are guarded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.guards.is_empty()
    }
}

impl Drop for EnvVarGuards {
    fn drop(&mut self) {
        while let Some(_guard) = self.guards.pop() {}
    }
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

    /// Set or remove `key` from an optional value and restore it on drop.
    #[must_use]
    pub fn set_optional(key: impl Into<String>, value: Option<impl Into<String>>) -> Self {
        match value {
            Some(value) => Self::set(key, value),
            None => Self::remove(key),
        }
    }

    /// Set or remove many environment variables and restore them on drop.
    ///
    /// Environment variables are process-global. Tests using this helper should
    /// still run serially, use unique variable names, or use process isolation.
    #[must_use]
    pub fn set_many<I, K, V>(items: I) -> EnvVarGuards
    where
        I: IntoIterator<Item = (K, Option<V>)>,
        K: Into<String>,
        V: Into<String>,
    {
        EnvVarGuards {
            guards: items
                .into_iter()
                .map(|(key, value)| Self::set_optional(key, value))
                .collect(),
        }
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
    fn env_guard_sets_optional_and_many_values() {
        let first = "WORLD_TEST_LITE_ENV_GUARD_OPTIONAL_FIRST";
        let second = "WORLD_TEST_LITE_ENV_GUARD_OPTIONAL_SECOND";
        env::set_var(first, "before");
        env::remove_var(second);
        {
            let _guards = EnvVarGuard::set_many([(first, Some("after")), (second, None::<&str>)]);
            assert_eq!(env::var(first).unwrap(), "after");
            assert!(env::var(second).is_err());
        }
        assert_eq!(env::var(first).unwrap(), "before");
        assert!(env::var(second).is_err());
        env::remove_var(first);
    }

    #[test]
    fn env_guard_many_restores_duplicate_keys_in_reverse_order() {
        let key = "WORLD_TEST_LITE_ENV_GUARD_DUPLICATE_KEY";
        env::set_var(key, "before");
        {
            let _guards = EnvVarGuard::set_many([(key, Some("middle")), (key, Some("after"))]);
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
