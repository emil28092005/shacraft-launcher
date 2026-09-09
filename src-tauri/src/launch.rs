//! Builds and spawns the real `java` invocation for a merged launch
//! profile. The `${auth_*}` placeholders are filled from a `PlayerIdentity`,
//! which is either a real Microsoft-authenticated session (`msa::LoginResult`)
//! or an explicit offline account (`PlayerIdentity::Offline`). Offline mode is
//! never silently substituted for a Microsoft session.

use crate::mojang::{self, MergedVersion};
use crate::session::PlayerIdentity;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fmt, fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug)]
pub enum LaunchError {
    Io(io::Error),
}

impl fmt::Display for LaunchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "cannot launch Minecraft: {error}"),
        }
    }
}

impl From<io::Error> for LaunchError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub struct LaunchRequest<'a> {
    pub java_executable: &'a Path,
    /// Shared vanilla+NeoForge files: versions/, libraries/, assets/.
    pub game_dir: &'a Path,
    /// ShaCraft-managed mods/config for this profile; becomes `--gameDir`
    /// so worlds/screenshots/config the player creates land there, not in
    /// the shared `game_dir`.
    pub profile_dir: &'a Path,
    pub merged: &'a MergedVersion,
    pub identity: &'a PlayerIdentity,
    pub memory_mb: u16,
    pub log_path: &'a Path,
    /// Short-lived onboarding grant; never persisted or placed in command-line arguments.
    pub onboarding_token: Option<&'a str>,
}

fn classpath_separator() -> &'static str {
    if cfg!(target_os = "windows") {
        ";"
    } else {
        ":"
    }
}

fn unique_classpath_entries(mut entries: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    entries.retain(|path| seen.insert(path.clone()));
    entries
}

fn build_classpath(game_dir: &Path, merged: &MergedVersion, client_jar: &Path) -> String {
    let no_features = HashMap::new();
    let mut entries: Vec<PathBuf> = merged
        .libraries
        .iter()
        .filter(|library| mojang::rule_allows(&library.rules, &no_features))
        .filter_map(|library| {
            library
                .downloads
                .as_ref()
                .and_then(|downloads| downloads.artifact.as_ref())
        })
        .map(|artifact| game_dir.join("libraries").join(&artifact.path))
        .collect();
    entries.push(client_jar.to_path_buf());
    // NeoForge's inherited profile can repeat vanilla libraries verbatim.
    // Passing the same jar twice makes SecureJarHandler abort during startup
    // (for example on gson-2.10.1.jar), so preserve order and keep each path
    // only once.
    let entries = unique_classpath_entries(entries);
    entries
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(classpath_separator())
}

/// A persistent-but-not-security-sensitive per-install identifier for the
/// `${clientid}` launch argument (Microsoft telemetry only, unrelated to
/// auth). Deliberately avoids adding a `uuid`/`rand` dependency for this:
/// it's hashed from time/process entropy via the `sha2` we already depend
/// on, formatted as a version-4-shaped UUID.
fn launcher_client_id(game_dir: &Path) -> Result<String, LaunchError> {
    let path = game_dir.join(".shacraft-client-id");
    if let Ok(existing) = fs::read_to_string(&path) {
        let trimmed = existing.trim();
        if trimmed.len() == 36 {
            return Ok(trimmed.to_string());
        }
    }
    fs::create_dir_all(game_dir)?;
    let generated = random_uuid_v4();
    fs::write(&path, &generated)?;
    Ok(generated)
}

static UUID_COUNTER: AtomicU64 = AtomicU64::new(0);

fn random_uuid_v4() -> String {
    let mut hasher = Sha256::new();
    hasher.update(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_le_bytes(),
    );
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(UUID_COUNTER.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    let stack_marker = 0_u8;
    hasher.update((&stack_marker as *const u8 as usize).to_le_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[0..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // RFC 4122 variant
    let hex = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    crate::session::format_uuid_with_dashes(&hex)
}

fn substitute(template: &str, vars: &HashMap<&str, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in vars {
        let token = format!("${{{key}}}");
        if result.contains(&token) {
            result = result.replace(&token, value);
        }
    }
    result
}

/// Java's argument-file syntax is independent of the platform shell. Keeping
/// the large JVM/module/classpath portion in an argfile avoids Windows'
/// 32,767 UTF-16 command-line limit while leaving account tokens out of it.
fn quote_argfile_argument(argument: &str) -> String {
    let mut quoted = String::with_capacity(argument.len() + 2);
    quoted.push('"');
    for character in argument.chars() {
        match character {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

fn write_jvm_argfile(path: &Path, arguments: &[String]) -> io::Result<()> {
    let mut contents = arguments
        .iter()
        .map(|argument| quote_argfile_argument(argument))
        .collect::<Vec<_>>()
        .join("\n");
    contents.push('\n');
    fs::write(path, contents)
}

/// Builds the full `java` command line for `request.merged` and spawns it
/// detached, with stdout/stderr both redirected to `request.log_path`.
/// Never blocks on the child exiting — the caller decides how to observe
/// that (see `lib.rs`'s launch command, which watches it on a background
/// thread and emits an event).
pub fn launch(request: &LaunchRequest) -> Result<Child, LaunchError> {
    fs::create_dir_all(request.profile_dir)?;
    let natives_dir = mojang::natives_directory(request.game_dir, &request.merged.id);
    fs::create_dir_all(&natives_dir)?;
    let assets_root = request.game_dir.join("assets");
    let libraries_dir = request.game_dir.join("libraries");
    let client_jar =
        mojang::client_jar_path(request.game_dir, &request.merged.client_jar_version_id);
    let classpath = build_classpath(request.game_dir, request.merged, &client_jar);

    let mut vars: HashMap<&str, String> = HashMap::new();
    vars.insert("auth_player_name", request.identity.name().to_string());
    // NeoForge's inherited JVM profile uses `${version_name}.jar` in
    // `-DignoreList`. The actual client jar belongs to the vanilla parent
    // (`1.21.1.jar`), not to the child profile (`neoforge-...`), so this
    // token must identify the parent or both vanilla and patched Minecraft
    // modules are loaded and Java aborts with a ResolutionException.
    vars.insert("version_name", request.merged.client_jar_version_id.clone());
    vars.insert("game_directory", request.profile_dir.display().to_string());
    vars.insert("assets_root", assets_root.display().to_string());
    vars.insert("assets_index_name", request.merged.asset_index.id.clone());
    vars.insert("auth_uuid", request.identity.uuid());
    vars.insert(
        "auth_access_token",
        request.identity.access_token().to_string(),
    );
    vars.insert("clientid", launcher_client_id(request.game_dir)?);
    vars.insert("auth_xuid", request.identity.xuid().to_string());
    vars.insert("user_type", request.identity.user_type().to_string());
    vars.insert("version_type", "ShaCraft Launcher".to_string());
    vars.insert("natives_directory", natives_dir.display().to_string());
    vars.insert("launcher_name", "ShaCraft Launcher".to_string());
    vars.insert("launcher_version", env!("CARGO_PKG_VERSION").to_string());
    vars.insert("classpath", classpath);
    vars.insert("library_directory", libraries_dir.display().to_string());
    vars.insert("classpath_separator", classpath_separator().to_string());

    let no_features = HashMap::new();
    let jvm_args = mojang::resolve_arguments(&request.merged.jvm_arguments, &no_features)
        .into_iter()
        .map(|argument| substitute(&argument, &vars))
        .collect::<Vec<_>>();
    let game_args = mojang::resolve_arguments(&request.merged.game_arguments, &no_features);

    let mut command = Command::new(request.java_executable);
    let memory_argument = format!("-Xmx{}M", request.memory_mb);
    if cfg!(windows) {
        let argfile = request.profile_dir.join(".shacraft-jvm.args");
        let mut argfile_arguments = Vec::with_capacity(jvm_args.len() + 1);
        argfile_arguments.push(memory_argument);
        argfile_arguments.extend(jvm_args);
        write_jvm_argfile(&argfile, &argfile_arguments)?;
        command.arg(format!("@{}", argfile.display()));
    } else {
        command.arg(memory_argument);
        command.args(jvm_args);
    }
    command.arg(&request.merged.main_class);
    for argument in game_args {
        command.arg(substitute(&argument, &vars));
    }
    command.current_dir(request.profile_dir);
    // Do not inherit a stale grant from the launcher process environment.
    command.env_remove("SHACRAFT_ONBOARDING_TOKEN");
    if let Some(token) = request.onboarding_token {
        command.env("SHACRAFT_ONBOARDING_TOKEN", token);
    }
    command.stdin(Stdio::null());

    let log_file = fs::File::create(request.log_path)?;
    command.stdout(Stdio::from(log_file.try_clone()?));
    command.stderr(Stdio::from(log_file));

    Ok(command.spawn()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_rfc4122_version_4_uuids() {
        let id = random_uuid_v4();
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[2].chars().next().unwrap(), '4');
        assert!(matches!(
            parts[3].chars().next().unwrap(),
            '8' | '9' | 'a' | 'b'
        ));
    }

    #[test]
    fn client_id_is_persisted_across_calls() {
        let dir = std::env::temp_dir().join(format!(
            "shacraft-launch-clientid-test-{}",
            std::process::id()
        ));
        let first = launcher_client_id(&dir).unwrap();
        let second = launcher_client_id(&dir).unwrap();
        assert_eq!(first, second);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn substitutes_known_tokens_only() {
        let mut vars = HashMap::new();
        vars.insert("auth_player_name", "Steve".to_string());
        assert_eq!(substitute("--username", &vars), "--username");
        assert_eq!(substitute("${auth_player_name}", &vars), "Steve");
        assert_eq!(
            substitute("-Djava.library.path=${natives_directory}", &vars),
            "-Djava.library.path=${natives_directory}"
        );
    }

    #[test]
    fn classpath_entries_are_unique() {
        let entries = unique_classpath_entries(vec![
            PathBuf::from("gson.jar"),
            PathBuf::from("gson.jar"),
            PathBuf::from("client.jar"),
        ]);
        assert_eq!(
            entries,
            vec![PathBuf::from("gson.jar"), PathBuf::from("client.jar")]
        );
    }

    #[test]
    fn quotes_java_argfile_arguments() {
        assert_eq!(
            quote_argfile_argument(r#"-Dpath=C:\\Users\\Jane Doe\\game"#),
            r#""-Dpath=C:\\\\Users\\\\Jane Doe\\\\game""#
        );
        assert_eq!(
            quote_argfile_argument(r#"-Dname="ShaCraft""#),
            r#""-Dname=\"ShaCraft\"""#
        );
    }
}
