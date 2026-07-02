//! Product-neutral password hashing and password-policy helpers.
//!
//! This crate owns reusable mechanics only: Argon2 hashing/verification,
//! password length/common-password checks, and optional breached-password SHA-1
//! file lookup. Products still own signup/login routes, account records,
//! enumeration-avoidance responses, captcha policy, and email flows.

use std::{
    error::Error,
    fmt,
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const DEFAULT_MIN_PASSWORD_CHARS: usize = 1;
const DEFAULT_MAX_PASSWORD_CHARS: usize = 256;
const DEFAULT_MIN_BREACH_FILE_BYTES: u64 = 1;
const DEFAULT_LINEAR_SCAN_MAX_BYTES: u64 = 64 * 1024 * 1024;

const COMMON_PASSWORDS: &[&str] = &[
    "123456",
    "123456789",
    "qwerty",
    "password",
    "password1",
    "password12",
    "password123",
    "password1234",
    "password12345",
    "password123456",
    "password1234567",
    "password12345678",
    "password123456789",
    "letmein",
    "letmein123456",
    "welcome",
    "welcome123456",
    "admin",
    "admin12345678",
    "administrator",
    "iloveyou",
    "iloveyou12345",
    "football",
    "football12345",
    "baseball12345",
    "monkey",
    "monkey123456",
    "dragon",
    "dragon123456",
    "sunshine",
    "sunshine12345",
    "princess",
    "princess1234",
    "qwerty123456",
    "qwertyuiop123",
    "123456789012",
    "1234567890123",
    "12345678901234",
    "123456789012345",
    "1234567890ab",
    "trustno112345",
    "starwars1234",
    "passw0rd1234",
    "abc123456789",
    "chairman",
    "airline",
];

/// Password helper error.
#[derive(Debug)]
pub enum PasswordError {
    /// Password was empty.
    Empty,
    /// Password was shorter than the configured minimum character count.
    TooShort {
        /// Character count.
        chars: usize,
        /// Configured minimum.
        min: usize,
    },
    /// Password exceeded the configured maximum character count.
    TooLong {
        /// Character count.
        chars: usize,
        /// Configured maximum.
        max: usize,
    },
    /// Password is in the built-in common password denylist.
    Common,
    /// Password appears in a breached-password SHA-1 list.
    Breached,
    /// Password hash could not be parsed or verified.
    InvalidHash,
    /// Password hashing failed.
    Hash(String),
    /// Breached password file lookup failed.
    BreachFile(String),
    /// Async worker failed.
    Worker(String),
}

impl fmt::Display for PasswordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("password is empty"),
            Self::TooShort { chars, min } => {
                write!(f, "password has {chars} characters; minimum is {min}")
            }
            Self::TooLong { chars, max } => {
                write!(f, "password has {chars} characters; maximum is {max}")
            }
            Self::Common => f.write_str("password is too common"),
            Self::Breached => f.write_str("password appears in a breached-password list"),
            Self::InvalidHash => f.write_str("password hash is invalid"),
            Self::Hash(error) => write!(f, "password hashing failed: {error}"),
            Self::BreachFile(error) => write!(f, "breached password file lookup failed: {error}"),
            Self::Worker(error) => write!(f, "password worker failed: {error}"),
        }
    }
}

impl Error for PasswordError {}

/// Common-password normalization mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommonPasswordNormalization {
    /// Trim outer whitespace and lowercase ASCII letters.
    TrimLowercase,
    /// Keep only ASCII alphanumeric characters and lowercase the result.
    ///
    /// This matches stricter web-account policies that reject variants such as
    /// `password-123456` when `password123456` is denied.
    AsciiAlphanumericLowercase,
}

/// Password validation policy.
#[derive(Debug, Clone)]
pub struct PasswordPolicy {
    /// Minimum Unicode scalar count accepted before hashing.
    pub min_chars: usize,
    /// Maximum Unicode scalar count accepted before hashing.
    pub max_chars: usize,
    /// Reject passwords in the built-in common password denylist.
    pub reject_common: bool,
    /// Product-supplied common passwords to deny in addition to the built-ins.
    pub additional_common_passwords: Vec<String>,
    /// How common-password inputs and denylist entries are normalized.
    pub common_password_normalization: CommonPasswordNormalization,
    /// Optional SHA-1 breached-password file.
    pub breached_sha1_file: Option<PathBuf>,
    /// Whether the SHA-1 file is sorted by hash.
    pub breached_sha1_file_sorted: bool,
    /// Minimum accepted breach-file size.
    pub breached_sha1_min_bytes: u64,
    /// Maximum bytes for linear scans of unsorted breach files.
    pub linear_scan_max_bytes: u64,
}

impl Default for PasswordPolicy {
    fn default() -> Self {
        Self {
            min_chars: DEFAULT_MIN_PASSWORD_CHARS,
            max_chars: DEFAULT_MAX_PASSWORD_CHARS,
            reject_common: true,
            additional_common_passwords: Vec::new(),
            common_password_normalization: CommonPasswordNormalization::TrimLowercase,
            breached_sha1_file: None,
            breached_sha1_file_sorted: true,
            breached_sha1_min_bytes: DEFAULT_MIN_BREACH_FILE_BYTES,
            linear_scan_max_bytes: DEFAULT_LINEAR_SCAN_MAX_BYTES,
        }
    }
}

impl PasswordPolicy {
    /// Validate a password against this policy.
    ///
    /// # Errors
    ///
    /// Returns [`PasswordError`] for policy violations or breach-file lookup
    /// failures.
    pub fn validate(&self, password: &str) -> Result<(), PasswordError> {
        validate_password_length_range(password, self.min_chars, self.max_chars)?;
        if self.reject_common
            && is_common_password_with_policy(
                password,
                self.common_password_normalization,
                &self.additional_common_passwords,
            )
        {
            return Err(PasswordError::Common);
        }
        if let Some(path) = &self.breached_sha1_file {
            let options = BreachedPasswordFileOptions {
                sorted: self.breached_sha1_file_sorted,
                min_bytes: self.breached_sha1_min_bytes,
                linear_scan_max_bytes: self.linear_scan_max_bytes,
            };
            if breached_password_file_contains(path, &password_sha1_hex(password), options)? {
                return Err(PasswordError::Breached);
            }
        }
        Ok(())
    }
}

/// Options for SHA-1 breached-password file lookup.
#[derive(Debug, Clone, Copy)]
pub struct BreachedPasswordFileOptions {
    /// Whether the file is sorted lexicographically by SHA-1 hash.
    pub sorted: bool,
    /// Minimum file size in bytes.
    pub min_bytes: u64,
    /// Maximum bytes allowed for linear scans of unsorted files.
    pub linear_scan_max_bytes: u64,
}

impl Default for BreachedPasswordFileOptions {
    fn default() -> Self {
        Self {
            sorted: true,
            min_bytes: DEFAULT_MIN_BREACH_FILE_BYTES,
            linear_scan_max_bytes: DEFAULT_LINEAR_SCAN_MAX_BYTES,
        }
    }
}

/// Return SHA-1(password) as uppercase hexadecimal.
#[must_use]
pub fn password_sha1_hex(password: &str) -> String {
    use sha1::Digest as _;
    let mut hasher = sha1::Sha1::new();
    hasher.update(password.as_bytes());
    hex::encode_upper(hasher.finalize())
}

/// Validate password length.
///
/// # Errors
///
/// Returns [`PasswordError::Empty`] or [`PasswordError::TooLong`].
pub fn validate_password_length(password: &str, max_chars: usize) -> Result<(), PasswordError> {
    validate_password_length_range(password, DEFAULT_MIN_PASSWORD_CHARS, max_chars)
}

/// Validate password length against inclusive minimum/maximum character counts.
///
/// # Errors
///
/// Returns [`PasswordError::Empty`], [`PasswordError::TooShort`], or
/// [`PasswordError::TooLong`].
pub fn validate_password_length_range(
    password: &str,
    min_chars: usize,
    max_chars: usize,
) -> Result<(), PasswordError> {
    let chars = password.chars().count();
    if chars == 0 {
        return Err(PasswordError::Empty);
    }
    if chars < min_chars {
        return Err(PasswordError::TooShort {
            chars,
            min: min_chars,
        });
    }
    if chars > max_chars {
        return Err(PasswordError::TooLong {
            chars,
            max: max_chars,
        });
    }
    Ok(())
}

/// Return whether a password is in the built-in common-password denylist.
#[must_use]
pub fn is_common_password(password: &str) -> bool {
    is_common_password_with_policy(
        password,
        CommonPasswordNormalization::TrimLowercase,
        std::iter::empty::<&str>(),
    )
}

/// Return whether a password is common with caller-supplied policy entries.
#[must_use]
pub fn is_common_password_with_policy(
    password: &str,
    normalization: CommonPasswordNormalization,
    additional_common_passwords: impl IntoIterator<Item = impl AsRef<str>>,
) -> bool {
    let normalized = normalize_common_password(password, normalization);
    if normalized.is_empty() {
        return false;
    }
    if COMMON_PASSWORDS
        .iter()
        .any(|common| normalize_common_password(common, normalization) == normalized)
    {
        return true;
    }
    additional_common_passwords
        .into_iter()
        .any(|common| normalize_common_password(common.as_ref(), normalization) == normalized)
}

fn normalize_common_password(password: &str, normalization: CommonPasswordNormalization) -> String {
    match normalization {
        CommonPasswordNormalization::TrimLowercase => password.trim().to_ascii_lowercase(),
        CommonPasswordNormalization::AsciiAlphanumericLowercase => password
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase(),
    }
}

/// Hash a password using Argon2's default profile and a random salt.
///
/// # Errors
///
/// Returns [`PasswordError`] if hashing fails.
pub fn hash_password(plain: &str) -> Result<String, PasswordError> {
    use argon2::{
        password_hash::{rand_core::OsRng, SaltString},
        Argon2, PasswordHasher,
    };
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(plain.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| PasswordError::Hash(error.to_string()))
}

/// Verify a password against an Argon2 PHC hash.
///
/// Malformed hashes return `Ok(false)` so products can keep login errors
/// enumeration-safe while separately auditing corrupted account rows.
///
/// # Errors
///
/// Currently returns no backend errors; the result shape is reserved for future
/// verification backends.
pub fn verify_password(hash: &str, plain: &str) -> Result<bool, PasswordError> {
    use argon2::{password_hash::PasswordHash, Argon2, PasswordVerifier};
    let Ok(hash) = PasswordHash::new(hash) else {
        return Ok(false);
    };
    Ok(Argon2::default()
        .verify_password(plain.as_bytes(), &hash)
        .is_ok())
}

/// Hash a password on a blocking worker thread.
///
/// # Errors
///
/// Returns [`PasswordError`] when hashing or the worker fails.
#[cfg(feature = "async")]
pub async fn hash_password_async(plain: String) -> Result<String, PasswordError> {
    tokio::task::spawn_blocking(move || hash_password(&plain))
        .await
        .map_err(|error| PasswordError::Worker(error.to_string()))?
}

/// Verify a password on a blocking worker thread.
///
/// # Errors
///
/// Returns [`PasswordError`] when verification or the worker fails.
#[cfg(feature = "async")]
pub async fn verify_password_async(hash: String, plain: String) -> Result<bool, PasswordError> {
    tokio::task::spawn_blocking(move || verify_password(&hash, &plain))
        .await
        .map_err(|error| PasswordError::Worker(error.to_string()))?
}

/// Validate breach-file metadata and sortedness.
///
/// # Errors
///
/// Returns [`PasswordError`] for missing, too-small, malformed, or unsorted
/// files.
pub fn validate_breached_password_file(
    path: &Path,
    options: BreachedPasswordFileOptions,
) -> Result<(), PasswordError> {
    validate_breach_file_metadata(path, options)?;

    let file = File::open(path).map_err(io_error)?;
    let mut previous_hash: Option<String> = None;
    let mut valid_entries = 0_u64;
    for (idx, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(io_error)?;
        if line.trim().is_empty() {
            return Err(PasswordError::BreachFile(format!(
                "blank breached password SHA-1 entry on line {}",
                idx + 1
            )));
        }
        let hash = breached_hash_from_line(&line).ok_or_else(|| {
            PasswordError::BreachFile(format!(
                "invalid breached password SHA-1 entry on line {}",
                idx + 1
            ))
        })?;
        if options.sorted {
            if let Some(previous_hash) = previous_hash.as_ref() {
                if hash < *previous_hash {
                    return Err(PasswordError::BreachFile(format!(
                        "breached password SHA-1 file is not sorted at line {}",
                        idx + 1
                    )));
                }
            }
        }
        previous_hash = Some(hash);
        valid_entries += 1;
    }
    if valid_entries == 0 {
        return Err(PasswordError::BreachFile(
            "breached password SHA-1 file contains no valid hashes".to_owned(),
        ));
    }
    Ok(())
}

/// Return whether a SHA-1 hash exists in a breach file.
///
/// # Errors
///
/// Returns [`PasswordError`] for file or format failures.
pub fn breached_password_file_contains(
    path: &Path,
    password_hash: &str,
    options: BreachedPasswordFileOptions,
) -> Result<bool, PasswordError> {
    validate_breach_file_metadata(path, options)?;
    let password_hash = normalize_sha1_hash(password_hash)?;
    if options.sorted {
        breached_password_hash_sorted_file_contains(path, &password_hash)
    } else {
        breached_password_hash_linear_scan_contains(path, &password_hash, options)
    }
}

fn validate_breach_file_metadata(
    path: &Path,
    options: BreachedPasswordFileOptions,
) -> Result<(), PasswordError> {
    let metadata = std::fs::metadata(path).map_err(io_error)?;
    if !metadata.is_file() {
        return Err(PasswordError::BreachFile(
            "breached password SHA-1 path is not a file".to_owned(),
        ));
    }
    let len = metadata.len();
    if len < options.min_bytes {
        return Err(PasswordError::BreachFile(format!(
            "breached password SHA-1 file is {len} bytes; minimum is {}",
            options.min_bytes
        )));
    }
    if !options.sorted && len > options.linear_scan_max_bytes {
        return Err(PasswordError::BreachFile(format!(
            "breached password SHA-1 file is {len} bytes; linear scan limit is {} bytes",
            options.linear_scan_max_bytes
        )));
    }
    Ok(())
}

fn breached_hash_from_line(line: &str) -> Option<String> {
    let hash = line
        .split_once(':')
        .map(|(hash, _count)| hash)
        .unwrap_or(line)
        .trim();
    normalize_sha1_hash(hash).ok()
}

fn normalize_sha1_hash(value: &str) -> Result<String, PasswordError> {
    let value = value.trim();
    if value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(value.to_ascii_uppercase())
    } else {
        Err(PasswordError::BreachFile(
            "invalid breached password SHA-1 hash".to_owned(),
        ))
    }
}

fn line_hash_cmp(line: &str, password_hash: &str) -> Result<std::cmp::Ordering, PasswordError> {
    let hash = breached_hash_from_line(line).ok_or_else(|| {
        PasswordError::BreachFile("invalid breached password SHA-1 entry".to_owned())
    })?;
    Ok(hash.as_str().cmp(password_hash))
}

fn breached_password_hash_sorted_file_contains(
    path: &Path,
    password_hash: &str,
) -> Result<bool, PasswordError> {
    let mut reader = BufReader::new(File::open(path).map_err(io_error)?);
    let len = reader.get_ref().metadata().map_err(io_error)?.len();
    if len == 0 {
        return Ok(false);
    }

    let mut low = 0_u64;
    let mut high = len;
    let mut line = String::new();
    while low < high {
        let mid = low + (high - low) / 2;
        let line_start = seek_probe_line_start(&mut reader, mid, len)?;
        if line_start >= high {
            reader.seek(SeekFrom::Start(low)).map_err(io_error)?;
            line.clear();
            if reader.read_line(&mut line).map_err(io_error)? == 0 {
                return Ok(false);
            }
            return Ok(line_hash_cmp(&line, password_hash)? == std::cmp::Ordering::Equal);
        }

        line.clear();
        let read = reader.read_line(&mut line).map_err(io_error)?;
        if read == 0 {
            return Ok(false);
        }

        match line_hash_cmp(&line, password_hash)? {
            std::cmp::Ordering::Equal => return Ok(true),
            std::cmp::Ordering::Less => {
                low = reader.stream_position().map_err(io_error)?;
            }
            std::cmp::Ordering::Greater => {
                high = line_start;
            }
        }
    }

    reader.seek(SeekFrom::Start(low)).map_err(io_error)?;
    line.clear();
    if reader.read_line(&mut line).map_err(io_error)? > 0 {
        return Ok(line_hash_cmp(&line, password_hash)? == std::cmp::Ordering::Equal);
    }
    Ok(false)
}

fn seek_probe_line_start(
    reader: &mut BufReader<File>,
    mid: u64,
    len: u64,
) -> Result<u64, PasswordError> {
    if mid == 0 {
        reader.seek(SeekFrom::Start(0)).map_err(io_error)?;
        return Ok(0);
    }
    if mid >= len {
        return Ok(len);
    }

    reader.seek(SeekFrom::Start(mid)).map_err(io_error)?;
    let mut current = [0_u8; 1];
    reader.read_exact(&mut current).map_err(io_error)?;
    if current[0] == b'\n' {
        return Ok(mid + 1);
    }

    let mut pos = mid;
    while pos > 0 {
        let previous_pos = pos - 1;
        reader
            .seek(SeekFrom::Start(previous_pos))
            .map_err(io_error)?;
        let mut previous = [0_u8; 1];
        reader.read_exact(&mut previous).map_err(io_error)?;
        if previous[0] == b'\n' {
            return Ok(pos);
        }
        pos = previous_pos;
    }
    reader.seek(SeekFrom::Start(0)).map_err(io_error)?;
    Ok(0)
}

fn breached_password_hash_linear_scan_contains(
    path: &Path,
    password_hash: &str,
    options: BreachedPasswordFileOptions,
) -> Result<bool, PasswordError> {
    validate_breach_file_metadata(path, options)?;
    let file = File::open(path).map_err(io_error)?;
    for (idx, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(io_error)?;
        if line.trim().is_empty() {
            continue;
        }
        let hash = breached_hash_from_line(&line).ok_or_else(|| {
            PasswordError::BreachFile(format!(
                "invalid breached password SHA-1 entry on line {}",
                idx + 1
            ))
        })?;
        if hash == password_hash {
            return Ok(true);
        }
    }
    Ok(false)
}

fn io_error(error: std::io::Error) -> PasswordError {
    PasswordError::BreachFile(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_verifies_argon2_passwords() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(hash.starts_with("$argon2"));
        assert!(verify_password(&hash, "correct horse battery staple").unwrap());
        assert!(!verify_password(&hash, "wrong").unwrap());
        assert!(!verify_password("not-a-phc-hash", "wrong").unwrap());
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn async_wrappers_use_blocking_workers() {
        let hash = hash_password_async("secret".to_owned()).await.unwrap();
        assert!(verify_password_async(hash, "secret".to_owned())
            .await
            .unwrap());
    }

    #[test]
    fn policy_rejects_empty_overlong_and_common_passwords() {
        let policy = PasswordPolicy::default();
        assert!(matches!(policy.validate(""), Err(PasswordError::Empty)));
        assert!(matches!(
            policy.validate(&"x".repeat(DEFAULT_MAX_PASSWORD_CHARS + 1)),
            Err(PasswordError::TooLong { .. })
        ));
        assert!(matches!(
            policy.validate("password123"),
            Err(PasswordError::Common)
        ));
        assert!(policy.validate("unique enough passphrase 123").is_ok());
    }

    #[test]
    fn policy_supports_minimum_length_and_product_common_passwords() {
        let policy = PasswordPolicy {
            min_chars: 12,
            additional_common_passwords: vec!["customproduct123".to_owned()],
            common_password_normalization: CommonPasswordNormalization::AsciiAlphanumericLowercase,
            ..PasswordPolicy::default()
        };
        assert!(matches!(
            policy.validate("short"),
            Err(PasswordError::TooShort { chars: 5, min: 12 })
        ));
        assert!(matches!(
            policy.validate("password-123456"),
            Err(PasswordError::Common)
        ));
        assert!(matches!(
            policy.validate("custom product 123"),
            Err(PasswordError::Common)
        ));
    }

    #[test]
    fn breach_file_lookup_supports_sorted_and_count_suffixed_files() {
        let candidate = "sample breach phrase";
        let breached_hash = password_sha1_hex(candidate);
        let mut hashes = [
            "0000000000000000000000000000000000000000".to_owned(),
            format!("{breached_hash}:42"),
            "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF".to_owned(),
        ];
        hashes.sort();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("breaches.txt");
        std::fs::write(&path, hashes.join("\n")).unwrap();

        assert!(
            validate_breached_password_file(&path, BreachedPasswordFileOptions::default()).is_ok()
        );
        assert!(breached_password_file_contains(
            &path,
            &breached_hash,
            BreachedPasswordFileOptions::default()
        )
        .unwrap());
        assert!(!breached_password_file_contains(
            &path,
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            BreachedPasswordFileOptions::default()
        )
        .unwrap());

        let policy = PasswordPolicy {
            breached_sha1_file: Some(path),
            ..PasswordPolicy::default()
        };
        assert!(matches!(
            policy.validate(candidate),
            Err(PasswordError::Breached)
        ));
    }

    #[test]
    fn sorted_breach_file_validation_rejects_blank_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("breaches.txt");
        std::fs::write(
            &path,
            "0000000000000000000000000000000000000000\n\nFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF\n",
        )
        .unwrap();

        let error = validate_breached_password_file(&path, BreachedPasswordFileOptions::default())
            .expect_err("sorted breach files should not contain blank searchable lines");
        assert!(error.to_string().contains("blank"));
        assert!(error.to_string().contains("line 2"));
    }

    #[test]
    fn sorted_breach_file_lookup_preserves_line_start_bounds() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("breaches.txt");
        let hashes = [
            "0000000000000000000000000000000000000000",
            "1111111111111111111111111111111111111111",
            "2222222222222222222222222222222222222222",
            "3333333333333333333333333333333333333333",
        ];
        std::fs::write(&path, format!("{}\n", hashes.join("\n"))).unwrap();

        assert!(
            validate_breached_password_file(&path, BreachedPasswordFileOptions::default()).is_ok()
        );
        for hash in hashes {
            assert!(
                breached_password_file_contains(
                    &path,
                    hash,
                    BreachedPasswordFileOptions::default()
                )
                .unwrap(),
                "expected sorted lookup to find {hash}"
            );
        }
        assert!(!breached_password_file_contains(
            &path,
            "4444444444444444444444444444444444444444",
            BreachedPasswordFileOptions::default()
        )
        .unwrap());
    }

    #[test]
    fn breach_file_lookup_can_scan_unsorted_small_files() {
        let candidate = "linear breach phrase";
        let breached_hash = password_sha1_hex(candidate);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("breaches.txt");
        std::fs::write(
            &path,
            format!("FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF\n{breached_hash}\n"),
        )
        .unwrap();

        assert!(breached_password_file_contains(
            &path,
            &breached_hash,
            BreachedPasswordFileOptions {
                sorted: false,
                ..BreachedPasswordFileOptions::default()
            }
        )
        .unwrap());
    }

    #[test]
    fn unsorted_breach_file_validation_checks_entries_without_sorting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("breaches.txt");
        std::fs::write(
            &path,
            "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF\n0000000000000000000000000000000000000000\n",
        )
        .unwrap();

        assert!(
            validate_breached_password_file(
                &path,
                BreachedPasswordFileOptions {
                    sorted: false,
                    ..BreachedPasswordFileOptions::default()
                },
            )
            .is_ok(),
            "unsorted validation should allow valid out-of-order hashes"
        );

        std::fs::write(
            &path,
            "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF\nnot-a-sha1\n",
        )
        .unwrap();
        let error = validate_breached_password_file(
            &path,
            BreachedPasswordFileOptions {
                sorted: false,
                ..BreachedPasswordFileOptions::default()
            },
        )
        .expect_err("unsorted validation should reject malformed lines");
        assert!(error.to_string().contains("line 2"));
    }

    #[test]
    fn breach_file_lookup_rejects_malformed_unsorted_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("breaches.txt");
        std::fs::write(
            &path,
            "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF\nnot-a-sha1\n",
        )
        .unwrap();

        let error = breached_password_file_contains(
            &path,
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            BreachedPasswordFileOptions {
                sorted: false,
                ..BreachedPasswordFileOptions::default()
            },
        )
        .expect_err("malformed nonblank line should fail closed");
        assert!(error.to_string().contains("line 2"));
    }

    #[test]
    fn breach_file_lookup_rejects_malformed_sorted_candidate_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("breaches.txt");
        std::fs::write(&path, "not-a-sha1\n").unwrap();

        let error = breached_password_file_contains(
            &path,
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            BreachedPasswordFileOptions::default(),
        )
        .expect_err("malformed candidate line should fail closed");
        assert!(error
            .to_string()
            .contains("invalid breached password SHA-1 entry"));
    }
}
