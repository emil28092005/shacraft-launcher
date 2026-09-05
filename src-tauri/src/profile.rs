use crate::manifest::{is_allowed_download_url, FilePolicy, ManagedFile, Manifest};
use reqwest::{blocking::Client, redirect::Policy};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{fmt, fs::{self, File}, io::{self, Read, Write}, path::{Path, PathBuf}};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInspection {
    pub root: String,
    pub managed_files: usize,
    pub missing_files: usize,
    pub mismatched_files: usize,
    pub up_to_date: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub root: String,
    pub downloaded_files: usize,
    pub reused_files: usize,
    pub downloaded_bytes: u64,
}

#[derive(Debug)]
pub enum ProfileError {
    Io(io::Error),
    Network(reqwest::Error),
    HttpStatus { path: String, status: reqwest::StatusCode },
    InvalidResponse { path: String, message: String },
    Integrity { path: String, message: String },
}

impl fmt::Display for ProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "Cannot inspect profile: {error}"),
            Self::Network(error) => write!(formatter, "Cannot download profile file: {error}"),
            Self::HttpStatus { path, status } => write!(formatter, "Download failed for {path}: server returned {status}"),
            Self::InvalidResponse { path, message } => write!(formatter, "Invalid response for {path}: {message}"),
            Self::Integrity { path, message } => write!(formatter, "Integrity check failed for {path}: {message}"),
        }
    }
}

pub fn inspect(root: &Path, manifest: &Manifest) -> Result<ProfileInspection, ProfileError> {
    let mut missing_files = 0;
    let mut mismatched_files = 0;

    for expected in &manifest.files {
        let path = root.join(&expected.path);
        let metadata = match path.metadata() {
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) => {
                mismatched_files += 1;
                continue;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing_files += 1;
                continue;
            }
            Err(error) => return Err(ProfileError::Io(error)),
        };

        if metadata.len() != expected.size || sha256(&path)? != expected.sha256.to_ascii_lowercase() {
            mismatched_files += 1;
        }
    }

    Ok(ProfileInspection {
        root: root.display().to_string(),
        managed_files: manifest.files.len(),
        missing_files,
        mismatched_files,
        up_to_date: missing_files == 0 && mismatched_files == 0,
    })
}

pub fn sync(root: &Path, manifest: &Manifest) -> Result<SyncResult, ProfileError> {
    let client = Client::builder()
        .redirect(Policy::custom(|attempt| {
            if is_allowed_download_url(attempt.url().as_str()) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(ProfileError::Network)?;

    let mut downloaded_files = 0;
    let mut reused_files = 0;
    let mut downloaded_bytes = 0;

    for expected in &manifest.files {
        let target = root.join(&expected.path);
        if matches!(expected.policy, FilePolicy::Seed) && target.exists() {
            reused_files += 1;
            continue;
        }
        if is_current(&target, expected)? {
            reused_files += 1;
            continue;
        }

        let bytes = download_file(&client, expected, &target)?;
        downloaded_files += 1;
        downloaded_bytes += bytes;
    }

    Ok(SyncResult {
        root: root.display().to_string(),
        downloaded_files,
        reused_files,
        downloaded_bytes,
    })
}

fn is_current(path: &Path, expected: &ManagedFile) -> Result<bool, ProfileError> {
    let metadata = match path.metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(ProfileError::Io(error)),
    };
    Ok(metadata.is_file() && metadata.len() == expected.size && sha256(path)? == expected.sha256.to_ascii_lowercase())
}

fn download_file(client: &Client, expected: &ManagedFile, target: &Path) -> Result<u64, ProfileError> {
    let parent = target.parent().ok_or_else(|| ProfileError::Integrity {
        path: expected.path.clone(),
        message: "target has no parent directory".into(),
    })?;
    fs::create_dir_all(parent).map_err(ProfileError::Io)?;

    let mut response = client.get(&expected.url).send().map_err(ProfileError::Network)?;
    if !response.status().is_success() {
        return Err(ProfileError::HttpStatus { path: expected.path.clone(), status: response.status() });
    }
    if let Some(length) = response.content_length() {
        if length != expected.size {
            return Err(ProfileError::InvalidResponse {
                path: expected.path.clone(),
                message: format!("expected {} bytes, received Content-Length {length}", expected.size),
            });
        }
    }

    let temporary = temp_path(target)?;
    let result = write_and_verify(&mut response, &temporary, expected);
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }

    fs::rename(&temporary, target).map_err(ProfileError::Io)?;
    Ok(expected.size)
}

fn temp_path(target: &Path) -> Result<PathBuf, ProfileError> {
    let file_name = target.file_name().and_then(|name| name.to_str()).ok_or_else(|| ProfileError::Integrity {
        path: target.display().to_string(),
        message: "target has no valid filename".into(),
    })?;
    Ok(target.with_file_name(format!(".{file_name}.shacraft.part")))
}

fn write_and_verify(response: &mut reqwest::blocking::Response, temporary: &Path, expected: &ManagedFile) -> Result<(), ProfileError> {
    let mut output = File::create(temporary).map_err(ProfileError::Io)?;
    let mut digest = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = response.read(&mut buffer).map_err(ProfileError::Io)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read]).map_err(ProfileError::Io)?;
        digest.update(&buffer[..read]);
        bytes += read as u64;
    }
    output.sync_all().map_err(ProfileError::Io)?;

    if bytes != expected.size {
        return Err(ProfileError::Integrity {
            path: expected.path.clone(),
            message: format!("expected {} bytes, downloaded {bytes}", expected.size),
        });
    }
    let actual = format!("{:x}", digest.finalize());
    if actual != expected.sha256.to_ascii_lowercase() {
        return Err(ProfileError::Integrity {
            path: expected.path.clone(),
            message: "SHA-256 does not match manifest".into(),
        });
    }
    Ok(())
}

fn sha256(path: &Path) -> Result<String, ProfileError> {
    let mut file = File::open(path).map_err(ProfileError::Io)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = file.read(&mut buffer).map_err(ProfileError::Io)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }

    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(test)]
mod tests {
    use super::inspect;
    use crate::manifest::{FilePolicy, Loader, ManagedFile, Manifest, Minecraft};
    use sha2::{Digest, Sha256};
    use std::{fs, process, time::{SystemTime, UNIX_EPOCH}};

    fn manifest(hash: String, size: u64) -> Manifest {
        Manifest {
            schema_version: 1,
            id: "aeronautics".into(),
            display_name: "Aeronautics".into(),
            minecraft: Minecraft {
                version: "1.21.1".into(),
                loader: Loader { kind: "neoforge".into(), version: "21.1.248".into() },
                java_major: 21,
            },
            files: vec![ManagedFile {
                path: "mods/example.jar".into(),
                url: "https://cdn.shacraft.ru/example.jar".into(),
                sha256: hash,
                size,
                policy: FilePolicy::Managed,
            }],
        }
    }

    #[test]
    fn reports_missing_and_matching_files() {
        let root = std::env::temp_dir().join(format!(
            "shacraft-launcher-test-{}-{}",
            process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let bytes = b"ShaCraft test file";
        let digest = format!("{:x}", Sha256::digest(bytes));
        let expected = manifest(digest, bytes.len() as u64);

        let missing = inspect(&root, &expected).unwrap();
        assert_eq!(missing.missing_files, 1);
        assert!(!missing.up_to_date);

        fs::create_dir_all(root.join("mods")).unwrap();
        fs::write(root.join("mods/example.jar"), bytes).unwrap();
        let current = inspect(&root, &expected).unwrap();
        assert_eq!(current.missing_files, 0);
        assert_eq!(current.mismatched_files, 0);
        assert!(current.up_to_date);

        fs::remove_dir_all(root).unwrap();
    }
}
