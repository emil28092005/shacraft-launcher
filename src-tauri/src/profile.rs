use crate::manifest::Manifest;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{fmt, fs::File, io::{self, Read}, path::Path};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInspection {
    pub root: String,
    pub managed_files: usize,
    pub missing_files: usize,
    pub mismatched_files: usize,
    pub up_to_date: bool,
}

#[derive(Debug)]
pub enum ProfileError {
    Io(io::Error),
}

impl fmt::Display for ProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "Cannot inspect profile: {error}"),
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
