use serde::Serialize;
use std::{env, path::PathBuf, process::Command};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaInstallation {
    pub executable: String,
    pub major: u8,
    pub version: String,
}

/// Finds a usable Java runtime without modifying the machine.
///
/// The launcher will later use this result to decide whether Java 21 needs to
/// be bundled. We deliberately check JAVA_HOME first: it is explicit and makes
/// development installations predictable across all supported platforms.
pub fn detect() -> Option<JavaInstallation> {
    candidates().into_iter().find_map(check_candidate)
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
    use super::{parse_major, parse_version};

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
