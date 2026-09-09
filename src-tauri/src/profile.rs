use crate::download::{self, Checksum, DownloadError};
use crate::manifest::{is_allowed_download_url, FilePolicy, ManagedFile, Manifest};
use reqwest::{blocking::Client, redirect::Policy};
use serde::Serialize;
use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

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
    Download { path: String, source: DownloadError },
    UnsafePath(PathBuf),
}

impl fmt::Display for ProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "Cannot access profile: {error}"),
            Self::Network(error) => write!(formatter, "Cannot download profile file: {error}"),
            Self::Download { path, source } => {
                write!(formatter, "Download failed for {path}: {source}")
            }
            Self::UnsafePath(path) => write!(
                formatter,
                "Profile path contains a symbolic link: {}",
                path.display()
            ),
        }
    }
}

pub fn inspect(root: &Path, manifest: &Manifest) -> Result<ProfileInspection, ProfileError> {
    let mut missing_files = 0;
    let mut mismatched_files = 0;

    for expected in &manifest.files {
        let path = managed_target(root, &expected.path)?;
        if !path.try_exists().map_err(ProfileError::Io)? {
            missing_files += 1;
            continue;
        }
        if matches!(expected.policy, FilePolicy::Seed) && path.is_file() {
            continue;
        }
        let checksum = Checksum::Sha256(expected.sha256.clone());
        if !download::is_current(&path, Some(expected.size), &checksum).map_err(ProfileError::Io)? {
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
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(10 * 60))
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 {
                attempt.error("too many redirects")
            } else if is_allowed_download_url(attempt.url().as_str()) {
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
        let target = managed_target(root, &expected.path)?;
        if matches!(expected.policy, FilePolicy::Seed) && target.is_file() {
            reused_files += 1;
            continue;
        }
        let checksum = Checksum::Sha256(expected.sha256.clone());
        if download::is_current(&target, Some(expected.size), &checksum)
            .map_err(ProfileError::Io)?
        {
            reused_files += 1;
            continue;
        }

        let bytes = download_managed_file(&client, expected, &target)?;
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

/// Reject pre-existing links in the managed subtree before inspecting or
/// replacing files. A signed relative path must not follow a local link into
/// an unrelated directory. This is not a sandbox against a hostile local user
/// changing directories concurrently under the launcher's OS identity.
fn managed_target(root: &Path, relative: &str) -> Result<PathBuf, ProfileError> {
    let mut path = root.to_path_buf();
    if let Some(parent) = root.parent() {
        reject_symlink(parent)?;
    }
    reject_symlink(&path)?;
    for component in relative.split('/') {
        path.push(component);
        reject_symlink(&path)?;
    }
    Ok(path)
}

fn reject_symlink(path: &Path) -> Result<(), ProfileError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(ProfileError::UnsafePath(path.to_path_buf()))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ProfileError::Io(error)),
    }
}

fn download_managed_file(
    client: &Client,
    expected: &ManagedFile,
    target: &Path,
) -> Result<u64, ProfileError> {
    let checksum = Checksum::Sha256(expected.sha256.clone());
    download::download_verified(
        client,
        &expected.url,
        target,
        Some(expected.size),
        &checksum,
        |_, _| {},
    )
    .map_err(|error| ProfileError::Download {
        path: expected.path.clone(),
        source: error,
    })
}

#[cfg(test)]
mod tests {
    use super::inspect;
    use crate::manifest::{FilePolicy, Loader, ManagedFile, Manifest, Minecraft};
    use sha2::{Digest, Sha256};
    use std::{
        fs, process,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn manifest(hash: String, size: u64) -> Manifest {
        Manifest {
            schema_version: 1,
            id: "aeronautics".into(),
            display_name: "Aeronautics".into(),
            minecraft: Minecraft {
                version: "1.21.1".into(),
                loader: Loader {
                    kind: "neoforge".into(),
                    version: "21.1.248".into(),
                },
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
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
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

    #[test]
    fn edited_seed_files_remain_up_to_date() {
        let root = std::env::temp_dir().join(format!(
            "shacraft-seed-test-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("mods")).unwrap();
        fs::write(root.join("mods/example.jar"), b"player's edits").unwrap();
        let mut expected = manifest("0".repeat(64), 42);
        expected.files[0].policy = FilePolicy::Seed;
        assert!(inspect(&root, &expected).unwrap().up_to_date);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn refuses_linked_profile_directories() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!(
            "shacraft-link-test-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("outside")).unwrap();
        fs::create_dir_all(root.join("profile")).unwrap();
        symlink(root.join("outside"), root.join("profile/mods")).unwrap();
        assert!(matches!(
            inspect(&root.join("profile"), &manifest("0".repeat(64), 42)),
            Err(super::ProfileError::UnsafePath(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
