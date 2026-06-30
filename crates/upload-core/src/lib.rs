//! Product-neutral upload key, size, and content-type safety primitives.
//!
//! This crate does not implement S3/R2 clients, bucket policies, image
//! processing, product asset rules, or UI flows.

use std::{error::Error, fmt, str::FromStr};

/// Default maximum object-key length in bytes.
pub const DEFAULT_OBJECT_KEY_MAX_LEN: usize = 512;

/// Error returned by upload safety helpers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadError {
    /// A string value was blank.
    Empty {
        /// Field name.
        field: &'static str,
    },
    /// An object key had unsafe syntax.
    InvalidObjectKey,
    /// A content type is not in the allowed set.
    UnsupportedContentType(String),
    /// The declared upload size was zero.
    EmptyUpload,
    /// The declared upload size exceeded the configured maximum.
    UploadTooLarge {
        /// Declared size.
        size: u64,
        /// Maximum allowed size.
        max: u64,
    },
}

impl fmt::Display for UploadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { field } => write!(f, "{field} is empty"),
            Self::InvalidObjectKey => f.write_str("object key is invalid"),
            Self::UnsupportedContentType(content_type) => {
                write!(f, "unsupported upload content type {content_type}")
            }
            Self::EmptyUpload => f.write_str("upload size must be positive"),
            Self::UploadTooLarge { size, max } => {
                write!(f, "upload size {size} exceeds maximum {max}")
            }
        }
    }
}

impl Error for UploadError {}

/// Product-neutral upload content type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum UploadContentType {
    /// PNG image.
    Png,
    /// JPEG image.
    Jpeg,
    /// WebP image.
    Webp,
    /// GIF image.
    Gif,
    /// PDF document.
    Pdf,
}

impl UploadContentType {
    /// Stable MIME string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Gif => "image/gif",
            Self::Pdf => "application/pdf",
        }
    }

    /// Validate that this content type is in a product-provided allowed subset.
    ///
    /// # Errors
    ///
    /// Returns [`UploadError::UnsupportedContentType`] when `allowed` does not
    /// contain this content type.
    pub fn require_allowed(self, allowed: &[Self]) -> Result<Self, UploadError> {
        if allowed.contains(&self) {
            Ok(self)
        } else {
            Err(UploadError::UnsupportedContentType(
                self.as_str().to_owned(),
            ))
        }
    }

    /// Parse an exact registered content type without trimming,
    /// control-character normalization, or aliases.
    ///
    /// Use this for raw request/provider input when leading/trailing
    /// whitespace or embedded control characters should be rejected rather than
    /// normalized.
    ///
    /// # Errors
    ///
    /// Returns [`UploadError::UnsupportedContentType`] for blank, non-exact,
    /// alias, control-character, or unsupported values.
    pub fn parse_strict(value: &str) -> Result<Self, UploadError> {
        if value.is_empty() || value != value.trim() || value.chars().any(|ch| ch.is_control()) {
            return Err(UploadError::UnsupportedContentType(value.to_owned()));
        }
        match value.to_ascii_lowercase().as_str() {
            "image/png" => Ok(Self::Png),
            "image/jpeg" => Ok(Self::Jpeg),
            "image/webp" => Ok(Self::Webp),
            "image/gif" => Ok(Self::Gif),
            "application/pdf" => Ok(Self::Pdf),
            other => Err(UploadError::UnsupportedContentType(other.to_owned())),
        }
    }
}

impl fmt::Display for UploadContentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for UploadContentType {
    type Err = UploadError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "image/png" => Ok(Self::Png),
            "image/jpeg" | "image/jpg" => Ok(Self::Jpeg),
            "image/webp" => Ok(Self::Webp),
            "image/gif" => Ok(Self::Gif),
            "application/pdf" => Ok(Self::Pdf),
            other => Err(UploadError::UnsupportedContentType(other.to_owned())),
        }
    }
}

/// Maximum upload size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UploadSizeLimit {
    max_bytes: u64,
}

impl UploadSizeLimit {
    /// Construct a size limit.
    ///
    /// # Errors
    ///
    /// Returns [`UploadError::EmptyUpload`] when `max_bytes` is zero.
    pub const fn new(max_bytes: u64) -> Result<Self, UploadError> {
        if max_bytes == 0 {
            return Err(UploadError::EmptyUpload);
        }
        Ok(Self { max_bytes })
    }

    /// Maximum bytes.
    #[must_use]
    pub const fn max_bytes(self) -> u64 {
        self.max_bytes
    }

    /// Validate a declared upload size.
    ///
    /// # Errors
    ///
    /// Returns [`UploadError`] if the size is zero or above the limit.
    pub const fn validate(self, size: u64) -> Result<(), UploadError> {
        if size == 0 {
            return Err(UploadError::EmptyUpload);
        }
        if size > self.max_bytes {
            return Err(UploadError::UploadTooLarge {
                size,
                max: self.max_bytes,
            });
        }
        Ok(())
    }
}

/// Validated object key for a product-owned storage backend.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScopedObjectKey(String);

/// Object-key validation policy.
#[derive(Debug, Clone, Copy)]
pub struct ObjectKeyPolicy<'a> {
    /// Required object-key prefix.
    pub required_prefix: &'a str,
    /// Maximum key length in bytes.
    pub max_len: usize,
    /// Predicate for allowed object-key characters, including path separators.
    pub allowed_char: fn(char) -> bool,
}

impl<'a> ObjectKeyPolicy<'a> {
    /// Construct a conservative object-key policy.
    #[must_use]
    pub const fn new(required_prefix: &'a str) -> Self {
        Self {
            required_prefix,
            max_len: DEFAULT_OBJECT_KEY_MAX_LEN,
            allowed_char: conservative_object_key_char,
        }
    }

    /// Return a copy with a different maximum key length.
    #[must_use]
    pub const fn with_max_len(mut self, max_len: usize) -> Self {
        self.max_len = max_len;
        self
    }

    /// Return a copy with a product-provided character policy.
    ///
    /// The predicate is applied to every character in the key and required
    /// prefix, including any `/` separators.
    #[must_use]
    pub const fn with_allowed_char(mut self, allowed_char: fn(char) -> bool) -> Self {
        self.allowed_char = allowed_char;
        self
    }
}

impl ScopedObjectKey {
    /// Validate an object key and require the configured prefix.
    ///
    /// # Errors
    ///
    /// Returns [`UploadError`] for blank keys, path traversal, leading slash,
    /// control characters, or missing prefix.
    pub fn new(value: impl AsRef<str>, required_prefix: &str) -> Result<Self, UploadError> {
        Self::new_with_policy(value, ObjectKeyPolicy::new(required_prefix))
    }

    /// Validate an object key against a product-provided policy.
    ///
    /// Object keys are exact storage values and are not trimmed or normalized.
    ///
    /// # Errors
    ///
    /// Returns [`UploadError`] for blank keys, path traversal, leading slash,
    /// backslashes, control characters, missing prefix, invalid characters, or
    /// length above the configured maximum.
    pub fn new_with_policy(
        value: impl AsRef<str>,
        policy: ObjectKeyPolicy<'_>,
    ) -> Result<Self, UploadError> {
        let value = value.as_ref();
        let required_prefix = policy.required_prefix;
        if value.is_empty() {
            return Err(UploadError::Empty {
                field: "object_key",
            });
        }
        if required_prefix.is_empty() {
            return Err(UploadError::Empty {
                field: "required_prefix",
            });
        }
        if policy.max_len == 0 || value.len() > policy.max_len {
            return Err(UploadError::InvalidObjectKey);
        }
        if invalid_key_syntax(required_prefix, policy.allowed_char)
            || value.starts_with('/')
            || value.ends_with('/')
            || value.contains("//")
            || value.split('/').any(|part| matches!(part, "." | ".." | ""))
            || value.chars().any(char::is_control)
            || value.contains('\\')
            || !value.chars().all(policy.allowed_char)
            || !has_prefix_boundary(value, required_prefix)
        {
            return Err(UploadError::InvalidObjectKey);
        }
        Ok(Self(value.to_owned()))
    }

    /// Access the object key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Conservative object-key character policy.
#[must_use]
pub const fn conservative_object_key_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '/' | '_' | '-' | '.')
}

fn invalid_key_syntax(value: &str, allowed_char: fn(char) -> bool) -> bool {
    value.starts_with('/')
        || value.contains("//")
        || value.split('/').any(|part| matches!(part, "." | ".."))
        || value.chars().any(char::is_control)
        || value.contains('\\')
        || !value.chars().all(allowed_char)
}

fn has_prefix_boundary(value: &str, required_prefix: &str) -> bool {
    value.strip_prefix(required_prefix).is_some_and(|rest| {
        rest.is_empty() || required_prefix.ends_with('/') || rest.starts_with('/')
    })
}

/// Upload completion state vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UploadCompletionState {
    /// Presign was issued but completion was not recorded.
    Pending,
    /// Product verified and recorded the uploaded object.
    Completed,
    /// Product invalidated the uploaded object reference.
    Deleted,
    /// Product rejected the uploaded object after verification.
    Rejected,
}

impl UploadCompletionState {
    /// Return whether no further normal upload work is expected for this state.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Deleted | Self::Rejected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_keys_require_prefix_and_reject_traversal() {
        assert!(ScopedObjectKey::new("uploads/account-1/logo.png", "uploads/account-1/").is_ok());
        assert!(ScopedObjectKey::new("uploads/account-1/logo.png", "uploads/account-1").is_ok());
        assert!(ScopedObjectKey::new("uploads/account-2/logo.png", "uploads/account-1/").is_err());
        assert!(ScopedObjectKey::new("uploads/account-10/logo.png", "uploads/account-1").is_err());
        assert!(ScopedObjectKey::new("uploads/account-1/../secret", "uploads/account-1/").is_err());
        assert!(ScopedObjectKey::new("uploads/account-1/logo.png", "uploads/../").is_err());
        assert!(ScopedObjectKey::new(" uploads/account-1/logo.png", "uploads/account-1/").is_err());
        assert!(ScopedObjectKey::new("uploads\\account-1\\logo.png", "uploads").is_err());
        assert!(ScopedObjectKey::new("uploads/account-1/logo.svg", "uploads/account-1/").is_ok());
    }

    #[test]
    fn object_key_policy_controls_length_and_characters() {
        fn image_key_char(ch: char) -> bool {
            ch.is_ascii_alphanumeric() || matches!(ch, '/' | '_' | '-' | '.')
        }

        let policy = ObjectKeyPolicy::new("uploads/account-1/")
            .with_max_len("uploads/account-1/logo.png".len())
            .with_allowed_char(image_key_char);
        assert!(ScopedObjectKey::new_with_policy("uploads/account-1/logo.png", policy).is_ok());
        assert!(
            ScopedObjectKey::new_with_policy("uploads/account-1/very-long-logo.png", policy)
                .is_err()
        );
        assert!(ScopedObjectKey::new_with_policy("uploads/account-1/logo png", policy).is_err());
    }

    #[test]
    fn size_limit_rejects_empty_and_large_uploads() {
        let limit = UploadSizeLimit::new(1024).unwrap();
        assert!(limit.validate(1).is_ok());
        assert!(limit.validate(0).is_err());
        assert!(limit.validate(1025).is_err());
    }

    #[test]
    fn content_types_can_be_restricted_to_product_subsets() {
        let airline_images = [
            UploadContentType::Png,
            UploadContentType::Jpeg,
            UploadContentType::Webp,
            UploadContentType::Gif,
        ];
        assert!(UploadContentType::Png
            .require_allowed(&airline_images)
            .is_ok());
        assert!(UploadContentType::Pdf
            .require_allowed(&airline_images)
            .is_err());
    }

    #[test]
    fn strict_content_type_rejects_control_characters_and_whitespace() {
        assert_eq!(
            UploadContentType::parse_strict("image/png"),
            Ok(UploadContentType::Png)
        );
        assert!(UploadContentType::parse_strict(" image/png").is_err());
        assert!(UploadContentType::parse_strict("image/png ").is_err());
        assert!(UploadContentType::parse_strict("image/png\n").is_err());
        assert!(UploadContentType::parse_strict("image/png\r").is_err());
        assert!(UploadContentType::parse_strict("image/png\0").is_err());
        assert!(UploadContentType::parse_strict("image/jpg").is_err());
        assert_eq!(
            "image/jpg".parse::<UploadContentType>(),
            Ok(UploadContentType::Jpeg)
        );
    }
}
