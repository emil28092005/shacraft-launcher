use crate::download::ProgressCallback;
use crate::runtime::{self, RuntimeError};
use reqwest::blocking::Client;
use serde::Serialize;
use std::{
    env, fmt,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaInstallation {
    pub executable: String,
    pub major: u8,
    pub version: String,
}

#[derive(Debug)]
pub enum EnsureJavaError {
    Provisioning(RuntimeError),
    ProvisionedButUnrecognised(PathBuf),
}

impl fmt::Display for EnsureJavaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Provisioning(error) => {
                write!(formatter, "Cannot install a Java runtime: {error}")
            }
            Self::ProvisionedButUnrecognised(path) => {
                write!(
                    formatter,
                    "Installed a Java runtime at {path:?}, but it did not report a usable version"
                )
            }
        }
    }
}

/// Finds a usable Java runtime without modifying the machine.
///
/// The launcher will later use this result to decide whether Java 21 needs to
/// be bundled. We deliberately check JAVA_HOME first: it is explicit and makes
/// development installations predictable across all supported platforms.
pub fn detect() -> Option<JavaInstallation> {
    candidates().into_iter().find_map(check_candidate)
}

/// Returns a Java runtime with exactly `required_major`, preferring a matching
/// installation already on the machine. Newer JVM majors are not assumed to
/// be compatible with the selected NeoForge/modpack version. Only downloads and extracts a
/// ShaCraft-managed Eclipse Temurin JRE under `runtime_root` (never touches
/// the user's own Java) when nothing suitable is already on the machine.
/// `on_progress` reports real download bytes when a JRE actually needs
/// fetching; it fires once with `(1, 1)` when an existing Java is reused.
pub fn ensure_java(
    client: &Client,
    runtime_root: &Path,
    required_major: u8,
    on_progress: &ProgressCallback,
) -> Result<JavaInstallation, EnsureJavaError> {
    if let Some(installation) = find_matching(candidates(), required_major, check_candidate) {
        on_progress(1, 1);
        return Ok(installation);
    }
    let executable = runtime::ensure_runtime(client, runtime_root, required_major, on_progress)
        .map_err(EnsureJavaError::Provisioning)?;
    check_candidate(executable.clone())
        .filter(|installation| installation.major == required_major)
        .ok_or(EnsureJavaError::ProvisionedButUnrecognised(executable))
}

fn find_matching(
    candidates: impl IntoIterator<Item = PathBuf>,
    required_major: u8,
    mut inspect: impl FnMut(PathBuf) -> Option<JavaInstallation>,
) -> Option<JavaInstallation> {
    candidates
        .into_iter()
        .filter_map(&mut inspect)
        .find(|installation| installation.major == required_major)
}

fn candidates() -> Vec<PathBuf> {
    let executable = if cfg!(target_os = "windows") {
        "java.exe"
    } else {
        "java"
    };

    let mut candidates = Vec::new();
    if let Some(java_home) = env::var_os("JAVA_HOME") {
        candidates.push(PathBuf::from(java_home).join("bin").join(executable));
    }
    candidates.push(PathBuf::from(executable));
    candidates
}

fn check_candidate(candidate: PathBuf) -> Option<JavaInstallation> {
    let output = Command::new(&candidate).arg("-version").output().ok()?;
    if !output.status.success() {
        return None;
    }

    let source = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let version = parse_version(&source)?;
    let major = parse_major(&version)?;
    Some(JavaInstallation {
        executable: candidate.display().to_string(),
        major,
        version,
    })
}

fn parse_version(source: &str) -> Option<String> {
    let marker = "version \"";
    let start = source.find(marker)? + marker.len();
    let remainder = &source[start..];
    let end = remainder.find('"')?;
    Some(remainder[..end].to_owned())
}

fn parse_major(version: &str) -> Option<u8> {
    let mut segments = version.split('.');
    let first = segments.next()?.parse::<u8>().ok()?;
    if first == 1 {
        segments.next()?.parse::<u8>().ok()
    } else {
        Some(first)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_path_runtime_is_not_hidden_by_wrong_java_home() {
        let candidates = ["JAVA_HOME", "PATH"].map(PathBuf::from);
        let found = find_matching(candidates, 21, |path| {
            let major = if path == Path::new("JAVA_HOME") {
                17
            } else {
                21
            };
            Some(JavaInstallation {
                executable: path.display().to_string(),
                major,
                version: major.to_string(),
            })
        })
        .unwrap();
        assert_eq!(found.executable, "PATH");
    }

    #[test]
    fn matching_home_remains_preferred_and_unusable_candidates_are_skipped() {
        let found = find_matching(["broken", "home", "path"].map(PathBuf::from), 21, |path| {
            assert_ne!(path, Path::new("path"), "must stop at matching JAVA_HOME");
            (path != Path::new("broken")).then(|| JavaInstallation {
                executable: path.display().to_string(),
                major: 21,
                version: "21".into(),
            })
        })
        .unwrap();
        assert_eq!(found.executable, "home");
        assert!(find_matching([PathBuf::from("newer")], 21, |path| Some(
            JavaInstallation {
                executable: path.display().to_string(),
                major: 25,
                version: "25".into()
            }
        ))
        .is_none());
    }

    #[test]
    fn parses_modern_java_version() {
        let output = "openjdk version \"21.0.8\" 2025-07-15";
        assert_eq!(parse_version(output).as_deref(), Some("21.0.8"));
        assert_eq!(parse_major("21.0.8"), Some(21));
    }

    #[test]
    fn parses_legacy_java_version() {
        assert_eq!(parse_major("1.8.0_452"), Some(8));
    }
}
