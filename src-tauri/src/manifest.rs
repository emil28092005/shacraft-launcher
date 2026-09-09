use serde::Deserialize;
use std::{collections::HashSet, fmt};

const MAX_MANIFEST_BYTES: usize = 2 * 1024 * 1024;
const CURRENT_SCHEMA_VERSION: u32 = 1;
const DOWNLOAD_HOSTS: [&str; 2] = ["shacraft.ru", "cdn.shacraft.ru"];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub schema_version: u32,
    pub id: String,
    pub display_name: String,
    pub minecraft: Minecraft,
    pub files: Vec<ManagedFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Minecraft {
    pub version: String,
    pub loader: Loader,
    pub java_major: u8,
}

#[derive(Debug, Deserialize)]
pub struct Loader {
    pub kind: String,
    pub version: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedFile {
    pub path: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub policy: FilePolicy,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FilePolicy {
    Managed,
    Seed,
}

#[derive(Debug)]
pub enum ManifestError {
    TooLarge,
    InvalidJson(serde_json::Error),
    Invalid(String),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(formatter, "Manifest is larger than 2 MiB"),
            Self::InvalidJson(error) => write!(formatter, "Invalid manifest JSON: {error}"),
            Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

pub fn validate_json(source: &str) -> Result<Manifest, ManifestError> {
    if source.len() > MAX_MANIFEST_BYTES {
        return Err(ManifestError::TooLarge);
    }

    let manifest = serde_json::from_str::<Manifest>(source).map_err(ManifestError::InvalidJson)?;
    validate(&manifest)?;
    Ok(manifest)
}

fn validate(manifest: &Manifest) -> Result<(), ManifestError> {
    if manifest.schema_version != CURRENT_SCHEMA_VERSION {
        return Err(ManifestError::Invalid(format!(
            "Unsupported schemaVersion {}; expected {CURRENT_SCHEMA_VERSION}",
            manifest.schema_version
        )));
    }
    if !is_identifier(&manifest.id) {
        return Err(ManifestError::Invalid(
            "Profile id must contain only lowercase letters, numbers and hyphens".into(),
        ));
    }
    if manifest.display_name.trim().is_empty() {
        return Err(ManifestError::Invalid(
            "Profile displayName cannot be empty".into(),
        ));
    }
    if !is_version(&manifest.minecraft.version)
        || manifest.minecraft.loader.kind.trim().is_empty()
        || !is_version(&manifest.minecraft.loader.version)
    {
        return Err(ManifestError::Invalid(
            "Minecraft version and loader must be specified".into(),
        ));
    }
    if !(8..=25).contains(&manifest.minecraft.java_major) {
        return Err(ManifestError::Invalid(
            "Unsupported Java major version".into(),
        ));
    }

    let mut paths = HashSet::new();
    for file in &manifest.files {
        match file.policy {
            FilePolicy::Managed | FilePolicy::Seed => {}
        }
        if !is_safe_relative_path(&file.path) {
            return Err(ManifestError::Invalid(format!(
                "Unsafe file path: {}",
                file.path
            )));
        }
        // A manifest must resolve to the same distinct files on Windows/macOS.
        if !paths.insert(file.path.to_lowercase()) {
            return Err(ManifestError::Invalid(format!(
                "Duplicate file path: {}",
                file.path
            )));
        }
        if !is_allowed_download_url(&file.url) {
            return Err(ManifestError::Invalid(format!(
                "File URL must use HTTPS and a ShaCraft host: {}",
                file.path
            )));
        }
        if file.size == 0 {
            return Err(ManifestError::Invalid(format!(
                "File has zero size: {}",
                file.path
            )));
        }
        if file.sha256.len() != 64 || !file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(ManifestError::Invalid(format!(
                "Invalid SHA-256 for {}",
                file.path
            )));
        }
    }
    Ok(())
}

fn is_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 48
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn is_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".-_".contains(&byte))
}

fn is_safe_relative_path(value: &str) -> bool {
    !value.is_empty() && value.split('/').all(is_portable_component)
}

pub(crate) fn is_portable_component(value: &str) -> bool {
    if value.is_empty()
        || value.ends_with(['.', ' '])
        || value
            .chars()
            .any(|ch| ch.is_control() || "\\:<>\"|?*".contains(ch))
    {
        return false;
    }
    let stem = value
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    !matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) && !(stem.len() == 4
        && (stem.starts_with("COM") || stem.starts_with("LPT"))
        && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}

pub(crate) fn is_allowed_download_url(value: &str) -> bool {
    crate::trusted_http::allows(value, &DOWNLOAD_HOSTS)
}

#[cfg(test)]
mod tests {
    use super::validate_json;

    const VALID: &str = r#"{
      "schemaVersion": 1,
      "id": "aeronautics",
      "displayName": "All of Create Aeronautics",
      "minecraft": { "version": "1.21.1", "loader": { "kind": "neoforge", "version": "21.1.248" }, "javaMajor": 21 },
      "files": [{
        "path": "mods/example.jar",
        "url": "https://cdn.shacraft.ru/aeronautics/example.jar",
        "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "size": 42,
        "policy": "managed"
      }]
    }"#;

    #[test]
    fn accepts_a_safe_manifest() {
        assert!(validate_json(VALID).is_ok());
    }

    #[test]
    fn rejects_path_traversal() {
        assert!(validate_json(&VALID.replace("mods/example.jar", "../secrets.txt")).is_err());
    }

    #[test]
    fn rejects_insecure_downloads() {
        assert!(validate_json(&VALID.replace("https://", "http://")).is_err());
    }

    #[test]
    fn rejects_third_party_download_hosts() {
        assert!(validate_json(&VALID.replace("cdn.shacraft.ru", "example.com")).is_err());
    }

    #[test]
    fn rejects_nonportable_paths_and_version_traversal() {
        for path in [
            "C:/escape.jar",
            "mods/file.jar:stream",
            "mods/CON.jar",
            "mods/LPT1",
            "mods/file.jar.",
            "mods/file.jar ",
            "mods//file.jar",
            "mods/../file.jar",
        ] {
            assert!(
                validate_json(&VALID.replace("mods/example.jar", path)).is_err(),
                "{path}"
            );
        }
        assert!(validate_json(&VALID.replace("21.1.248", "../../escape")).is_err());
    }

    #[test]
    fn rejects_ambiguous_download_authorities() {
        for host in [
            "user@cdn.shacraft.ru",
            "cdn.shacraft.ru:444",
            "cdn.shacraft.ru.evil.example",
        ] {
            assert!(validate_json(&VALID.replace("cdn.shacraft.ru", host)).is_err());
        }
    }
}
