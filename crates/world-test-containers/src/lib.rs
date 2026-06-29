//! Product-neutral Testcontainers image wrappers.
//!
//! This crate intentionally does not run product migrations, seed product data,
//! or define product-specific schemas. Product repositories own those steps.

#[cfg(feature = "postgres")]
pub mod postgres {
    //! Postgres image wrapper.

    use std::borrow::Cow;

    use testcontainers::{
        core::wait::LogWaitStrategy, core::CopyToContainer, core::WaitFor, Image,
    };

    /// Tagged official Postgres image for Docker-backed tests.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Postgres {
        tag: String,
    }

    impl Postgres {
        /// Create a Postgres image wrapper with an explicit tag.
        #[must_use]
        pub fn with_tag(tag: impl Into<String>) -> Self {
            Self { tag: tag.into() }
        }

        /// Return the configured image tag.
        #[must_use]
        pub fn tag_value(&self) -> &str {
            &self.tag
        }
    }

    impl Default for Postgres {
        fn default() -> Self {
            let tag = std::env::var("TEST_PG_TAG").unwrap_or_else(|_| "16-alpine".to_owned());
            Self { tag }
        }
    }

    impl Image for Postgres {
        fn name(&self) -> &str {
            "postgres"
        }

        fn tag(&self) -> &str {
            self.tag.as_str()
        }

        fn ready_conditions(&self) -> Vec<WaitFor> {
            // The official image logs this once for initdb's temporary server
            // and again for the final server. Waiting twice avoids starting
            // migrations against the temporary server.
            vec![WaitFor::log(
                LogWaitStrategy::stdout_or_stderr("database system is ready to accept connections")
                    .with_times(2),
            )]
        }

        fn env_vars(
            &self,
        ) -> impl IntoIterator<Item = (impl Into<Cow<'_, str>>, impl Into<Cow<'_, str>>)> {
            [
                ("POSTGRES_USER", "postgres"),
                ("POSTGRES_PASSWORD", "postgres"),
                ("POSTGRES_DB", "postgres"),
            ]
        }

        fn copy_to_sources(&self) -> impl IntoIterator<Item = &CopyToContainer> {
            std::iter::empty()
        }

        fn cmd(&self) -> impl IntoIterator<Item = impl Into<Cow<'_, str>>> {
            std::iter::empty::<Cow<'static, str>>()
        }
    }
}

#[cfg(feature = "valkey")]
pub mod valkey {
    //! Valkey image wrapper for Redis-protocol tests.

    use std::borrow::Cow;

    use testcontainers::{core::CopyToContainer, core::WaitFor, Image};

    /// Tagged official Valkey image for Docker-backed Redis-protocol tests.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Valkey {
        tag: String,
    }

    impl Valkey {
        /// Create a Valkey image wrapper with an explicit tag.
        #[must_use]
        pub fn with_tag(tag: impl Into<String>) -> Self {
            Self { tag: tag.into() }
        }

        /// Return the configured image tag.
        #[must_use]
        pub fn tag_value(&self) -> &str {
            &self.tag
        }
    }

    impl Default for Valkey {
        fn default() -> Self {
            let tag = std::env::var("TEST_REDIS_TAG").unwrap_or_else(|_| "8.1-bookworm".to_owned());
            Self { tag }
        }
    }

    impl Image for Valkey {
        fn name(&self) -> &str {
            "valkey/valkey"
        }

        fn tag(&self) -> &str {
            self.tag.as_str()
        }

        fn ready_conditions(&self) -> Vec<WaitFor> {
            vec![WaitFor::message_on_stdout("Ready to accept connections")]
        }

        fn env_vars(
            &self,
        ) -> impl IntoIterator<Item = (impl Into<Cow<'_, str>>, impl Into<Cow<'_, str>>)> {
            std::iter::empty::<(Cow<'static, str>, Cow<'static, str>)>()
        }

        fn copy_to_sources(&self) -> impl IntoIterator<Item = &CopyToContainer> {
            std::iter::empty()
        }

        fn cmd(&self) -> impl IntoIterator<Item = impl Into<Cow<'_, str>>> {
            std::iter::empty::<Cow<'static, str>>()
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "postgres")]
    #[test]
    fn postgres_image_uses_official_defaults() {
        use testcontainers::Image;

        let image = crate::postgres::Postgres::with_tag("16-alpine");
        assert_eq!(image.name(), "postgres");
        assert_eq!(image.tag(), "16-alpine");
        assert_eq!(image.ready_conditions().len(), 1);
    }

    #[cfg(feature = "valkey")]
    #[test]
    fn valkey_image_uses_official_defaults() {
        use testcontainers::Image;

        let image = crate::valkey::Valkey::with_tag("8.1-bookworm");
        assert_eq!(image.name(), "valkey/valkey");
        assert_eq!(image.tag(), "8.1-bookworm");
        assert_eq!(image.ready_conditions().len(), 1);
    }
}
