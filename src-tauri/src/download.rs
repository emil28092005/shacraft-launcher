use reqwest::blocking::{Client, Response};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::{
    fmt,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

/// A progress reporter shared across worker threads: `(bytes_done, bytes_total)`.
/// `bytes_total` may be a coarse estimate (e.g. item counts rather than bytes)
/// for callers that can't know exact sizes upfront, as long as it converges
/// to the true total by completion.
pub type ProgressCallback = Arc<dyn Fn(u64, u64) + Send + Sync>;

/// Expected content hash for a downloaded file. Mojang publishes SHA-1 for
/// game files; ShaCraft and everything else here uses SHA-256.
#[derive(Clone, Debug)]
pub enum Checksum {
    Sha1(String),
    Sha256(String),
}

impl Checksum {
    fn matches(&self, sha1_hex: &str, sha256_hex: &str) -> bool {
        match self {
            Self::Sha1(expected) => expected.eq_ignore_ascii_case(sha1_hex),
            Self::Sha256(expected) => expected.eq_ignore_ascii_case(sha256_hex),
        }
    }
}

#[derive(Debug)]
pub enum DownloadError {
    Io(io::Error),
    Network(reqwest::Error),
    HttpStatus(reqwest::StatusCode),
    SizeMismatch { expected: u64, actual: u64 },
    ChecksumMismatch,
    InvalidTargetPath,
}

impl fmt::Display for DownloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Network(error) => write!(formatter, "network error: {error}"),
            Self::HttpStatus(status) => write!(formatter, "server returned {status}"),
            Self::SizeMismatch { expected, actual } => {
                write!(formatter, "expected {expected} bytes, received {actual}")
            }
            Self::ChecksumMismatch => formatter.write_str("checksum does not match"),
            Self::InvalidTargetPath => formatter.write_str("target has no valid filename"),
        }
    }
}

/// Computes both SHA-1 and SHA-256 of a local file in a single pass, so a
/// caller can check whichever `Checksum` variant it needs without re-reading
/// the file.
pub fn file_hashes(path: &Path) -> io::Result<(String, String)> {
    let mut file = File::open(path)?;
    let mut sha1 = Sha1::new();
    let mut sha256 = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        sha1.update(&buffer[..read]);
        sha256.update(&buffer[..read]);
    }
    Ok((
        format!("{:x}", sha1.finalize()),
        format!("{:x}", sha256.finalize()),
    ))
}

/// True if `path` already exists, matches `expected_size` (when given) and
/// `checksum`. Used to skip re-downloading files that are already current.
pub fn is_current(
    path: &Path,
    expected_size: Option<u64>,
    checksum: &Checksum,
) -> io::Result<bool> {
    let metadata = match path.metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if !metadata.is_file() {
        return Ok(false);
    }
    if let Some(expected_size) = expected_size {
        if metadata.len() != expected_size {
            return Ok(false);
        }
    }
    let (sha1_hex, sha256_hex) = file_hashes(path)?;
    Ok(checksum.matches(&sha1_hex, &sha256_hex))
}

fn temp_path(target: &Path) -> Result<PathBuf, DownloadError> {
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(DownloadError::InvalidTargetPath)?;
    Ok(target.with_file_name(format!(".{file_name}.shacraft.part")))
}

/// Replaces `target` with a fully-written temporary sibling. Unix rename
/// replaces an existing file atomically, while Windows rename rejects an
/// existing destination. The backup dance keeps the old file recoverable if
/// the second rename fails (for example because antivirus briefly locks it).
pub(crate) fn replace_file(temporary: &Path, target: &Path) -> io::Result<()> {
    #[cfg(not(windows))]
    {
        fs::rename(temporary, target)
    }
    #[cfg(windows)]
    {
        if !target.exists() {
            return fs::rename(temporary, target);
        }
        let file_name = target
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "target has no valid filename")
            })?;
        let backup = target.with_file_name(format!(".{file_name}.shacraft.backup"));
        match fs::remove_file(&backup) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        fs::rename(target, &backup)?;
        if let Err(error) = fs::rename(temporary, target) {
            let _ = fs::rename(&backup, target);
            return Err(error);
        }
        let _ = fs::remove_file(backup);
        Ok(())
    }
}

/// Downloads `url` to `target`, verifying size (if known ahead of time) and
/// `checksum` before atomically renaming the temporary file into place.
/// `on_progress(downloaded_bytes, total_bytes)` is called after every chunk;
/// `total_bytes` is `None` when the server did not send `Content-Length`.
pub fn download_verified(
    client: &Client,
    url: &str,
    target: &Path,
    expected_size: Option<u64>,
    checksum: &Checksum,
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> Result<u64, DownloadError> {
    let parent = target.parent().ok_or(DownloadError::InvalidTargetPath)?;
    fs::create_dir_all(parent).map_err(DownloadError::Io)?;

    let mut response = client.get(url).send().map_err(DownloadError::Network)?;
    if !response.status().is_success() {
        return Err(DownloadError::HttpStatus(response.status()));
    }
    let total = expected_size.or_else(|| response.content_length());
    if let (Some(expected), Some(length)) = (expected_size, response.content_length()) {
        if expected != length {
            return Err(DownloadError::SizeMismatch {
                expected,
                actual: length,
            });
        }
    }

    let temporary = temp_path(target)?;
    let result = write_and_verify(
        &mut response,
        &temporary,
        expected_size,
        checksum,
        total,
        &mut on_progress,
    );
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    let bytes = result.unwrap();
    replace_file(&temporary, target).map_err(DownloadError::Io)?;
    Ok(bytes)
}

fn write_and_verify(
    response: &mut Response,
    temporary: &Path,
    expected_size: Option<u64>,
    checksum: &Checksum,
    total: Option<u64>,
    on_progress: &mut impl FnMut(u64, Option<u64>),
) -> Result<u64, DownloadError> {
    let mut output = File::create(temporary).map_err(DownloadError::Io)?;
    let mut sha1 = Sha1::new();
    let mut sha256 = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = response.read(&mut buffer).map_err(DownloadError::Io)?;
        if read == 0 {
            break;
        }
        output
            .write_all(&buffer[..read])
            .map_err(DownloadError::Io)?;
        sha1.update(&buffer[..read]);
        sha256.update(&buffer[..read]);
        bytes += read as u64;
        on_progress(bytes, total);
    }
    output.sync_all().map_err(DownloadError::Io)?;

    if let Some(expected) = expected_size {
        if bytes != expected {
            return Err(DownloadError::SizeMismatch {
                expected,
                actual: bytes,
            });
        }
    }
    let sha1_hex = format!("{:x}", sha1.finalize());
    let sha256_hex = format!("{:x}", sha256.finalize());
    if !checksum.matches(&sha1_hex, &sha256_hex) {
        return Err(DownloadError::ChecksumMismatch);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::{file_hashes, is_current, replace_file, Checksum};
    use std::{
        fs, process,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_file(contents: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "shacraft-download-test-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn computes_both_hashes() {
        let path = temp_file(b"hello shacraft");
        let (sha1_hex, sha256_hex) = file_hashes(&path).unwrap();
        assert_eq!(sha1_hex, "124b319646ec08b4fb2a2b65bbd21c0431b4eaf4");
        assert_eq!(
            sha256_hex,
            "d34eb8ea6396e8492109813c717f7eefd0437c10ff55a7b11949cfae900c946d"
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn is_current_checks_size_and_hash() {
        let path = temp_file(b"hello shacraft");
        let (_, sha256_hex) = file_hashes(&path).unwrap();
        assert!(is_current(&path, Some(14), &Checksum::Sha256(sha256_hex.clone())).unwrap());
        assert!(!is_current(&path, Some(13), &Checksum::Sha256(sha256_hex.clone())).unwrap());
        assert!(!is_current(&path, Some(14), &Checksum::Sha256("0".repeat(64))).unwrap());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn is_current_false_for_missing_file() {
        let path = std::env::temp_dir().join("shacraft-download-test-missing-file-xyz");
        assert!(!is_current(&path, None, &Checksum::Sha256("0".repeat(64))).unwrap());
    }

    #[test]
    fn replaces_an_existing_file() {
        let target = temp_file(b"old");
        let temporary = target.with_extension("replacement");
        fs::write(&temporary, b"new").unwrap();
        replace_file(&temporary, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert!(!temporary.exists());
        fs::remove_file(target).unwrap();
    }
}
