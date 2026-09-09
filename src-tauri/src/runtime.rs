//! Java runtime auto-provisioning via Eclipse Adoptium (Temurin), GPLv2+CE.
//!
//! Independent, hardcoded trust domain: `api.adoptium.net` only. The API
//! returns the release's SHA-256 inline, which we verify before extracting
//! anything. The user's own system Java is never touched — see
//! `java.rs::ensure_java`, which only calls into this module when no
//! sufficiently new Java is already installed.

use crate::download::{self, Checksum, DownloadError, ProgressCallback};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::{fmt, fs, io, path::{Path, PathBuf}};

const ADOPTIUM_HOST: &str = "api.adoptium.net";
const RUNTIME_HOSTS: [&str; 4] = [ADOPTIUM_HOST, "github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com"];

pub fn http_client() -> Result<Client, reqwest::Error> {
    crate::trusted_http::client(&RUNTIME_HOSTS, std::time::Duration::from_secs(10 * 60))
}

#[derive(Debug)]
pub enum RuntimeError {
    Network(reqwest::Error),
    HttpStatus(reqwest::StatusCode),
    NoRelease,
    UntrustedPackage,
    UnexpectedArchiveLayout,
    Download(DownloadError),
    Io(io::Error),
    Zip(zip::result::ZipError),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(error) => write!(formatter, "network error: {error}"),
            Self::HttpStatus(status) => write!(formatter, "Adoptium returned {status}"),
            Self::NoRelease => formatter.write_str("Adoptium has no matching JRE release for this platform"),
            Self::UntrustedPackage => formatter.write_str("Adoptium package has an unsafe archive name, URL or checksum"),
            Self::UnexpectedArchiveLayout => formatter.write_str("Java archive did not contain a single top-level directory as expected"),
            Self::Download(error) => write!(formatter, "{error}"),
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Zip(error) => write!(formatter, "zip error: {error}"),
        }
    }
}

impl From<reqwest::Error> for RuntimeError {
    fn from(error: reqwest::Error) -> Self {
        Self::Network(error)
    }
}
impl From<DownloadError> for RuntimeError {
    fn from(error: DownloadError) -> Self {
        Self::Download(error)
    }
}
impl From<io::Error> for RuntimeError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<zip::result::ZipError> for RuntimeError {
    fn from(error: zip::result::ZipError) -> Self {
        Self::Zip(error)
    }
}

#[derive(Debug, Deserialize)]
struct AdoptiumAsset {
    binary: AdoptiumBinary,
}

#[derive(Debug, Deserialize)]
struct AdoptiumBinary {
    package: AdoptiumPackage,
}

#[derive(Debug, Deserialize)]
struct AdoptiumPackage {
    link: String,
    checksum: String,
    name: String,
}

fn validate_package(package: &AdoptiumPackage) -> Result<(), RuntimeError> {
    if !crate::manifest::is_portable_component(&package.name)
        || package.name.contains('/')
        || !(package.name.ends_with(".tar.gz") || package.name.ends_with(".zip"))
        || !crate::trusted_http::allows(&package.link, &RUNTIME_HOSTS)
        || package.checksum.len() != 64
        || !package.checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(RuntimeError::UntrustedPackage);
    }
    Ok(())
}

fn adoptium_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "mac"
    } else {
        "linux"
    }
}

/// Adoptium's `architecture` query parameter values, which don't match
/// Rust's `std::env::consts::ARCH` strings 1:1 (notably `x86_64` -> `x64`).
fn adoptium_arch() -> Option<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Some("x64"),
        "aarch64" => Some("aarch64"),
        _ => None,
    }
}

fn java_executable_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "java.exe"
    } else {
        "java"
    }
}

pub fn java_path_in(runtime_dir: &Path) -> PathBuf {
    let mac_bundle = runtime_dir.join("Contents/Home/bin").join(java_executable_name());
    if mac_bundle.exists() {
        return mac_bundle;
    }
    runtime_dir.join("bin").join(java_executable_name())
}

/// Ensures a Java `major` runtime is available under
/// `runtime_root/<major>-<os>-<arch>/`, downloading and extracting it from
/// Adoptium if it isn't already there. Returns the path to the `java`
/// executable. Never touches any Java the user already has installed
/// elsewhere. `on_progress` reports real download bytes (extraction is fast
/// enough afterwards not to need its own progress).
pub fn ensure_runtime(client: &Client, runtime_root: &Path, major: u8, on_progress: &ProgressCallback) -> Result<PathBuf, RuntimeError> {
    let arch = adoptium_arch().ok_or(RuntimeError::NoRelease)?;
    let runtime_dir = runtime_root.join(format!("{major}-{}-{arch}", adoptium_os()));
    let marker = runtime_dir.join(".shacraft-complete");
    let java_path = java_path_in(&runtime_dir);
    if marker.exists() && java_path.exists() {
        on_progress(1, 1);
        return Ok(java_path);
    }

    let url = format!(
        "https://{ADOPTIUM_HOST}/v3/assets/latest/{major}/hotspot?image_type=jre&os={}&architecture={arch}&vendor=eclipse",
        adoptium_os()
    );
    let response = client.get(&url).send()?;
    if !response.status().is_success() {
        return Err(RuntimeError::HttpStatus(response.status()));
    }
    let assets: Vec<AdoptiumAsset> = response.json()?;
    let package = assets.into_iter().next().map(|asset| asset.binary.package).ok_or(RuntimeError::NoRelease)?;
    validate_package(&package)?;

    fs::create_dir_all(runtime_root)?;
    let archive_path = runtime_root.join(&package.name);
    let progress = on_progress.clone();
    download::download_verified(client, &package.link, &archive_path, None, &Checksum::Sha256(package.checksum), move |current, total| {
        progress(current, total.unwrap_or(current));
    })?;

    extract_single_root_archive(&archive_path, &runtime_dir)?;
    fs::remove_file(&archive_path).ok();
    fs::write(&marker, b"ok")?;

    let java_path = java_path_in(&runtime_dir);
    if !java_path.exists() {
        return Err(RuntimeError::UnexpectedArchiveLayout);
    }
    Ok(java_path)
}

/// Extracts a `.tar.gz` or `.zip` archive that contains exactly one
/// top-level directory (true of every Adoptium release archive), and
/// renames that directory into place as `target_dir`.
fn extract_single_root_archive(archive_path: &Path, target_dir: &Path) -> Result<(), RuntimeError> {
    let staging = target_dir.with_file_name(format!(
        "{}.staging",
        target_dir.file_name().and_then(|name| name.to_str()).unwrap_or("runtime")
    ));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;

    let is_zip = archive_path.extension().and_then(|extension| extension.to_str()) == Some("zip");
    if is_zip {
        let file = fs::File::open(archive_path)?;
        let mut archive = zip::ZipArchive::new(file)?;
        archive.extract(&staging)?;
    } else {
        let file = fs::File::open(archive_path)?;
        let decompressed = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(decompressed);
        archive.unpack(&staging)?;
    }

    let mut entries = fs::read_dir(&staging)?.collect::<Result<Vec<_>, io::Error>>()?;
    if entries.len() != 1 || !entries[0].file_type()?.is_dir() {
        fs::remove_dir_all(&staging).ok();
        return Err(RuntimeError::UnexpectedArchiveLayout);
    }
    let inner = entries.remove(0).path();
    if target_dir.exists() {
        fs::remove_dir_all(target_dir)?;
    }
    if let Some(parent) = target_dir.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(&inner, target_dir)?;
    fs::remove_dir_all(&staging).ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_path_uses_platform_executable_name() {
        let dir = Path::new("/tmp/example-runtime");
        let path = java_path_in(dir);
        assert!(path.ends_with(java_executable_name()));
    }

    fn package() -> AdoptiumPackage {
        AdoptiumPackage {
            name: "OpenJDK21U-jre_x64_linux_hotspot_21.0.8_9.tar.gz".into(),
            link: "https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.8%2B9/runtime.tar.gz".into(),
            checksum: "a".repeat(64),
        }
    }

    #[test]
    fn accepts_only_portable_runtime_archive_names() {
        assert!(validate_package(&package()).is_ok());
        let mut windows = package();
        windows.name = "OpenJDK21U-jre_x64_windows_hotspot.zip".into();
        assert!(validate_package(&windows).is_ok());
        for name in ["../runtime.tar.gz", "/runtime.zip", "C:\\runtime.zip", "runtime.zip:stream", "CON.zip", "LPT1.zip", "runtime.zip.", "runtime.zip ", "runtime.exe"] {
            let mut malicious = package();
            malicious.name = name.into();
            assert!(matches!(validate_package(&malicious), Err(RuntimeError::UntrustedPackage)), "{name}");
        }
    }

    #[test]
    fn rejects_untrusted_runtime_urls_and_invalid_hashes() {
        for link in ["http://github.com/runtime.zip", "https://evil.example/runtime.zip", "https://github.com.evil.example/runtime.zip", "https://user@github.com/runtime.zip"] {
            let mut malicious = package();
            malicious.link = link.into();
            assert!(validate_package(&malicious).is_err());
        }
        let mut malicious = package();
        malicious.checksum = "not-a-checksum".into();
        assert!(validate_package(&malicious).is_err());
        for host in RUNTIME_HOSTS {
            assert!(crate::trusted_http::allows(&format!("https://{host}/release.tar.gz"), &RUNTIME_HOSTS));
        }
    }

    /// Live smoke test: resolves the current platform's latest Temurin 21
    /// JRE from Adoptium, downloads it, verifies the checksum, and extracts
    /// it. Not run by default; `cargo test -- --ignored ensure_runtime`.
    #[test]
    #[ignore]
    fn live_provisions_java_21() {
        let client = Client::builder().build().unwrap();
        let root = std::env::temp_dir().join(format!("shacraft-runtime-live-{}", std::process::id()));
        let no_progress: ProgressCallback = std::sync::Arc::new(|_, _| {});
        let java = ensure_runtime(&client, &root, 21, &no_progress).unwrap();
        assert!(java.exists());

        let output = std::process::Command::new(&java).arg("-version").output().unwrap();
        assert!(output.status.success());

        // Second call must hit the "already provisioned" fast path.
        let java_again = ensure_runtime(&client, &root, 21, &no_progress).unwrap();
        assert_eq!(java, java_again);

        fs::remove_dir_all(&root).ok();
    }
}
