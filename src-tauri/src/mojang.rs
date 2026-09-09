//! Vanilla Minecraft trust boundary.
//!
//! Everything here talks only to Mojang's own public, unauthenticated CDN.
//! Which Minecraft version to install is decided entirely by the
//! ShaCraft-signed manifest (`manifest.rs`, `Manifest.minecraft`); this
//! module never takes a URL from that manifest. Every JSON document and
//! binary is verified against a SHA-1 obtained from an already-verified
//! parent document, all the way back to `version_manifest_v2.json`.
//!
//! The version-JSON types and `inheritsFrom` merge here follow the same
//! spec NeoForge's installer targets (see `neoforge.rs`), so a modloader
//! profile is handled by the identical, loader-agnostic merge algorithm
//! every vanilla-compatible third-party launcher uses.

use crate::download::{self, Checksum, DownloadError};
use reqwest::blocking::Client;
use serde::{de::DeserializeOwned, Deserialize};
use sha1::{Digest, Sha1};
use std::{
    collections::HashMap,
    fmt, fs, io,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

const VERSION_MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const MOJANG_HOSTS: [&str; 4] = [
    "piston-meta.mojang.com",
    "piston-data.mojang.com",
    "libraries.minecraft.net",
    "resources.download.minecraft.net",
];
// Asset downloads are latency-bound (tens of thousands of small files), not
// bandwidth-bound, so throughput scales with concurrent in-flight requests
// far more than with per-connection speed; Mojang's CDN comfortably handles
// this many. Raised from an earlier, overly conservative 12.
const ASSET_WORKERS: usize = 48;

pub fn is_allowed_host(url: &str) -> bool {
    crate::trusted_http::allows(url, &MOJANG_HOSTS)
}

pub fn http_client() -> Result<Client, reqwest::Error> {
    crate::trusted_http::client(&MOJANG_HOSTS, std::time::Duration::from_secs(10 * 60))
}

#[derive(Debug)]
pub enum MojangError {
    Network(reqwest::Error),
    HttpStatus(reqwest::StatusCode),
    InvalidJson(serde_json::Error),
    ChecksumMismatch(String),
    DisallowedHost(String),
    MissingField(String),
    Download(DownloadError),
    Io(io::Error),
}

impl fmt::Display for MojangError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(error) => write!(formatter, "network error: {error}"),
            Self::HttpStatus(status) => write!(formatter, "Mojang returned {status}"),
            Self::InvalidJson(error) => write!(formatter, "invalid Mojang JSON: {error}"),
            Self::ChecksumMismatch(context) => write!(formatter, "checksum mismatch for {context}"),
            Self::DisallowedHost(url) => write!(formatter, "URL is not a recognised Mojang host: {url}"),
            Self::MissingField(field) => write!(formatter, "version JSON is missing {field}"),
            Self::Download(error) => write!(formatter, "{error}"),
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
        }
    }
}

impl From<DownloadError> for MojangError {
    fn from(error: DownloadError) -> Self {
        Self::Download(error)
    }
}

impl From<io::Error> for MojangError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

// ---------------------------------------------------------------------
// Version manifest
// ---------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct VersionManifest {
    pub versions: Vec<VersionManifestEntry>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct VersionManifestEntry {
    pub id: String,
    pub url: String,
    pub sha1: String,
}

pub fn fetch_version_manifest(client: &Client) -> Result<VersionManifest, MojangError> {
    fetch_json(client, VERSION_MANIFEST_URL, None)
}

pub fn find_version<'a>(manifest: &'a VersionManifest, id: &str) -> Option<&'a VersionManifestEntry> {
    manifest.versions.iter().find(|entry| entry.id == id)
}

// ---------------------------------------------------------------------
// Version JSON (shared shape with NeoForge's installed profile JSON)
// ---------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    pub id: String,
    pub main_class: String,
    #[serde(default)]
    pub arguments: Option<Arguments>,
    #[serde(default)]
    pub asset_index: Option<AssetIndexRef>,
    #[serde(default)]
    pub downloads: Option<Downloads>,
    #[serde(default)]
    pub libraries: Vec<Library>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<ArgumentValue>,
    #[serde(default)]
    pub jvm: Vec<ArgumentValue>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum ArgumentValue {
    Plain(String),
    Conditional { rules: Vec<Rule>, value: StringOrList },
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum StringOrList {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndexRef {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Downloads {
    pub client: DownloadRef,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DownloadRef {
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LibraryDownloads {
    pub artifact: Option<Artifact>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Artifact {
    pub path: String,
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Rule {
    pub action: RuleAction,
    #[serde(default)]
    pub os: Option<RuleOs>,
    #[serde(default)]
    pub features: Option<HashMap<String, bool>>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Allow,
    Disallow,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct RuleOs {
    pub name: Option<String>,
    pub arch: Option<String>,
}

fn current_os_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

fn arch_matches(expected: &str) -> bool {
    let normalized = if expected == "arm64" { "aarch64" } else { expected };
    normalized == std::env::consts::ARCH
}

fn os_matches(os: &RuleOs) -> bool {
    if let Some(name) = &os.name {
        if name != current_os_name() {
            return false;
        }
    }
    if let Some(arch) = &os.arch {
        if !arch_matches(arch) {
            return false;
        }
    }
    true
}

fn features_match(required: &HashMap<String, bool>, active: &HashMap<String, bool>) -> bool {
    required.iter().all(|(key, value)| active.get(key).copied().unwrap_or(false) == *value)
}

/// Evaluates a Mojang-style rule list: no rules means always allowed;
/// otherwise the last matching rule (in order) decides, defaulting to
/// disallowed if nothing matched. `active_features` should list only the
/// optional launch features actually supported (today: none — no demo
/// mode, no custom resolution, no quick-play).
pub fn rule_allows(rules: &[Rule], active_features: &HashMap<String, bool>) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        let os_ok = rule.os.as_ref().is_none_or(os_matches);
        let features_ok = rule.features.as_ref().is_none_or(|required| features_match(required, active_features));
        if os_ok && features_ok {
            allowed = rule.action == RuleAction::Allow;
        }
    }
    allowed
}

/// Flattens an argument list into plain strings, dropping conditional
/// entries whose rules don't match this platform/feature set.
pub fn resolve_arguments(arguments: &[ArgumentValue], active_features: &HashMap<String, bool>) -> Vec<String> {
    let mut resolved = Vec::new();
    for argument in arguments {
        match argument {
            ArgumentValue::Plain(value) => resolved.push(value.clone()),
            ArgumentValue::Conditional { rules, value } => {
                if rule_allows(rules, active_features) {
                    match value {
                        StringOrList::One(value) => resolved.push(value.clone()),
                        StringOrList::Many(values) => resolved.extend(values.iter().cloned()),
                    }
                }
            }
        }
    }
    resolved
}

pub fn fetch_version_json(client: &Client, entry: &VersionManifestEntry) -> Result<VersionJson, MojangError> {
    fetch_json(client, &entry.url, Some(&entry.sha1))
}

fn fetch_json<T: DeserializeOwned>(client: &Client, url: &str, expected_sha1: Option<&str>) -> Result<T, MojangError> {
    if !is_allowed_host(url) {
        return Err(MojangError::DisallowedHost(url.to_string()));
    }
    let response = client.get(url).send().map_err(MojangError::Network)?;
    if !response.status().is_success() {
        return Err(MojangError::HttpStatus(response.status()));
    }
    let bytes = response.bytes().map_err(MojangError::Network)?;
    if let Some(expected) = expected_sha1 {
        let mut hasher = Sha1::new();
        hasher.update(&bytes);
        let actual = format!("{:x}", hasher.finalize());
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(MojangError::ChecksumMismatch(url.to_string()));
        }
    }
    serde_json::from_slice(&bytes).map_err(MojangError::InvalidJson)
}

// ---------------------------------------------------------------------
// inheritsFrom merge
// ---------------------------------------------------------------------

pub struct MergedVersion {
    /// The version being launched: the child's id when there is a loader
    /// (e.g. `neoforge-21.1.248`), otherwise the parent's. Used for
    /// `${version_name}` and as the name of the `versions/<id>/` directory
    /// logs and natives live under.
    pub id: String,
    /// The id whose `<id>.jar` actually exists on disk and belongs on the
    /// classpath — always the vanilla parent's, confirmed empirically: a
    /// NeoForge profile has no `versions/neoforge-<ver>/neoforge-<ver>.jar`
    /// of its own (FancyModLoader loads the patched client itself, see
    /// `neoforge.rs`), so callers must use this id, not `id` above, to find
    /// the client jar (`client_jar_path`).
    pub client_jar_version_id: String,
    pub main_class: String,
    pub game_arguments: Vec<ArgumentValue>,
    pub jvm_arguments: Vec<ArgumentValue>,
    pub libraries: Vec<Library>,
    pub asset_index: AssetIndexRef,
    pub client: DownloadRef,
}

/// Merges a child version JSON (e.g. NeoForge's) onto its vanilla parent
/// the same way the official Minecraft Launcher merges any `inheritsFrom`
/// profile: the child's `mainClass` wins, its arguments are appended after
/// the parent's, and its libraries are appended after the parent's.
/// `assetIndex`/`downloads.client` always come from the parent, since
/// modloader profiles don't redeclare them.
pub fn merge_versions(parent: &VersionJson, child: Option<&VersionJson>) -> Result<MergedVersion, MojangError> {
    let asset_index = parent.asset_index.clone().ok_or_else(|| MojangError::MissingField("assetIndex".into()))?;
    let client = parent
        .downloads
        .as_ref()
        .map(|downloads| downloads.client.clone())
        .ok_or_else(|| MojangError::MissingField("downloads.client".into()))?;
    let parent_arguments = parent.arguments.clone().unwrap_or_default();

    let Some(child) = child else {
        return Ok(MergedVersion {
            id: parent.id.clone(),
            client_jar_version_id: parent.id.clone(),
            main_class: parent.main_class.clone(),
            game_arguments: parent_arguments.game,
            jvm_arguments: parent_arguments.jvm,
            libraries: parent.libraries.clone(),
            asset_index,
            client,
        });
    };
    let child_arguments = child.arguments.clone().unwrap_or_default();

    let mut game_arguments = parent_arguments.game;
    game_arguments.extend(child_arguments.game);
    let mut jvm_arguments = parent_arguments.jvm;
    jvm_arguments.extend(child_arguments.jvm);
    let mut libraries = parent.libraries.clone();
    libraries.extend(child.libraries.clone());

    Ok(MergedVersion {
        id: child.id.clone(),
        client_jar_version_id: parent.id.clone(),
        main_class: child.main_class.clone(),
        game_arguments,
        jvm_arguments,
        libraries,
        asset_index,
        client,
    })
}

// ---------------------------------------------------------------------
// Downloading
// ---------------------------------------------------------------------

pub use crate::download::ProgressCallback;

pub fn natives_directory(game_dir: &Path, version_id: &str) -> PathBuf {
    game_dir.join("versions").join(version_id).join("natives")
}

pub fn client_jar_path(game_dir: &Path, version_id: &str) -> PathBuf {
    game_dir.join("versions").join(version_id).join(format!("{version_id}.jar"))
}

pub fn ensure_client_jar(client: &Client, game_dir: &Path, version_id: &str, download_ref: &DownloadRef) -> Result<PathBuf, MojangError> {
    if !is_allowed_host(&download_ref.url) {
        return Err(MojangError::DisallowedHost(download_ref.url.clone()));
    }
    let target = client_jar_path(game_dir, version_id);
    let checksum = Checksum::Sha1(download_ref.sha1.clone());
    if !download::is_current(&target, Some(download_ref.size), &checksum)? {
        download::download_verified(client, &download_ref.url, &target, Some(download_ref.size), &checksum, |_, _| {})?;
    }
    Ok(target)
}

/// Downloads every rule-allowed library with a `downloads.artifact`,
/// returning the resulting jar paths in the same order as `libraries`.
pub fn ensure_libraries(client: &Client, game_dir: &Path, libraries: &[Library], on_progress: &ProgressCallback) -> Result<Vec<PathBuf>, MojangError> {
    let mut paths = Vec::new();
    let mut tasks = Vec::new();
    for library in libraries {
        if !rule_allows(&library.rules, &HashMap::new()) {
            continue;
        }
        let Some(artifact) = library.downloads.as_ref().and_then(|downloads| downloads.artifact.as_ref()) else {
            continue;
        };
        let target = game_dir.join("libraries").join(&artifact.path);
        paths.push(target.clone());
        tasks.push(DownloadTask {
            url: artifact.url.clone(),
            target,
            size: artifact.size,
            checksum: Checksum::Sha1(artifact.sha1.clone()),
        });
    }
    download_many(client, tasks, on_progress)?;
    Ok(paths)
}

#[derive(Debug, Deserialize)]
pub struct AssetIndex {
    pub objects: HashMap<String, AssetObject>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

pub fn ensure_asset_index(client: &Client, game_dir: &Path, asset_index: &AssetIndexRef) -> Result<AssetIndex, MojangError> {
    if !is_allowed_host(&asset_index.url) {
        return Err(MojangError::DisallowedHost(asset_index.url.clone()));
    }
    let target = game_dir.join("assets").join("indexes").join(format!("{}.json", asset_index.id));
    let checksum = Checksum::Sha1(asset_index.sha1.clone());
    if !download::is_current(&target, Some(asset_index.size), &checksum)? {
        download::download_verified(client, &asset_index.url, &target, Some(asset_index.size), &checksum, |_, _| {})?;
    }
    let bytes = fs::read(&target)?;
    serde_json::from_slice(&bytes).map_err(MojangError::InvalidJson)
}

pub fn ensure_assets(client: &Client, game_dir: &Path, index: &AssetIndex, on_progress: &ProgressCallback) -> Result<(), MojangError> {
    let objects_dir = game_dir.join("assets").join("objects");
    let tasks = index
        .objects
        .values()
        .map(|object| {
            let prefix = &object.hash[0..2];
            DownloadTask {
                url: format!("https://resources.download.minecraft.net/{prefix}/{}", object.hash),
                target: objects_dir.join(prefix).join(&object.hash),
                size: object.size,
                checksum: Checksum::Sha1(object.hash.clone()),
            }
        })
        .collect();
    download_many(client, tasks, on_progress)
}

struct DownloadTask {
    url: String,
    target: PathBuf,
    size: u64,
    checksum: Checksum,
}

const MAX_DOWNLOAD_ATTEMPTS: u32 = 5;

/// Retries a single file a few times with a short backoff before giving up.
/// With tens of thousands of individual requests in `ensure_assets`, an
/// occasional transient failure (reset connection, one bad TLS record) is
/// expected network noise, not a reason to abort the whole install — this
/// is exactly what real launchers do at this scale.
fn download_with_retries(client: &Client, task: &DownloadTask) -> Result<u64, DownloadError> {
    let mut last_error = None;
    for attempt in 1..=MAX_DOWNLOAD_ATTEMPTS {
        match download::download_verified(client, &task.url, &task.target, Some(task.size), &task.checksum, |_, _| {}) {
            Ok(bytes) => return Ok(bytes),
            Err(error) => {
                last_error = Some(error);
                if attempt < MAX_DOWNLOAD_ATTEMPTS {
                    std::thread::sleep(std::time::Duration::from_millis(200 * attempt as u64));
                }
            }
        }
    }
    Err(last_error.expect("loop runs at least once"))
}

/// Downloads `tasks` using a small worker pool, calling `on_progress` with
/// cumulative (downloaded, total) bytes as each file completes. Stops
/// spawning new work once the first error is seen and returns it.
fn download_many(client: &Client, tasks: Vec<DownloadTask>, on_progress: &ProgressCallback) -> Result<(), MojangError> {
    let total: u64 = tasks.iter().map(|task| task.size).sum();
    if total == 0 {
        return Ok(());
    }
    let downloaded = AtomicU64::new(0);
    let queue = Mutex::new(tasks);
    let first_error: Mutex<Option<MojangError>> = Mutex::new(None);

    std::thread::scope(|scope| {
        for _ in 0..ASSET_WORKERS {
            let queue = &queue;
            let downloaded = &downloaded;
            let first_error = &first_error;
            let on_progress = Arc::clone(on_progress);
            scope.spawn(move || loop {
                if first_error.lock().unwrap().is_some() {
                    break;
                }
                let Some(task) = queue.lock().unwrap().pop() else { break };
                if !is_allowed_host(&task.url) {
                    *first_error.lock().unwrap() = Some(MojangError::DisallowedHost(task.url));
                    continue;
                }
                let already_current = download::is_current(&task.target, Some(task.size), &task.checksum).unwrap_or(false);
                if !already_current {
                    if let Err(error) = download_with_retries(client, &task) {
                        *first_error.lock().unwrap() = Some(MojangError::Download(error));
                        continue;
                    }
                }
                let done = downloaded.fetch_add(task.size, Ordering::SeqCst) + task.size;
                on_progress(done, total);
            });
        }
    });

    match first_error.into_inner().unwrap() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(action: RuleAction, os_name: Option<&str>) -> Rule {
        Rule {
            action,
            os: os_name.map(|name| RuleOs { name: Some(name.into()), arch: None }),
            features: None,
        }
    }

    #[test]
    fn empty_rules_always_allow() {
        assert!(rule_allows(&[], &HashMap::new()));
    }

    #[test]
    fn single_matching_os_rule_allows() {
        let rules = vec![rule(RuleAction::Allow, Some(current_os_name()))];
        assert!(rule_allows(&rules, &HashMap::new()));
    }

    #[test]
    fn non_matching_os_rule_disallows() {
        let other = if current_os_name() == "windows" { "linux" } else { "windows" };
        let rules = vec![rule(RuleAction::Allow, Some(other))];
        assert!(!rule_allows(&rules, &HashMap::new()));
    }

    #[test]
    fn unsupported_feature_is_excluded_by_default() {
        let mut features = HashMap::new();
        features.insert("is_demo_user".to_string(), true);
        let rules = vec![Rule { action: RuleAction::Allow, os: None, features: Some(features) }];
        // We never activate optional features, so a rule requiring one
        // must not match even though there's no OS constraint.
        assert!(!rule_allows(&rules, &HashMap::new()));
    }

    #[test]
    fn resolves_plain_and_conditional_arguments() {
        let args = vec![
            ArgumentValue::Plain("--username".into()),
            ArgumentValue::Plain("${auth_player_name}".into()),
            ArgumentValue::Conditional {
                rules: vec![rule(RuleAction::Allow, Some(current_os_name()))],
                value: StringOrList::Many(vec!["--this-os-only".into()]),
            },
            ArgumentValue::Conditional {
                rules: vec![rule(RuleAction::Allow, Some("nonexistent-os"))],
                value: StringOrList::One("--never".into()),
            },
        ];
        let resolved = resolve_arguments(&args, &HashMap::new());
        assert_eq!(resolved, vec!["--username", "${auth_player_name}", "--this-os-only"]);
    }

    #[test]
    fn merge_appends_child_after_parent() {
        let parent: VersionJson = serde_json::from_str(
            r#"{
                "id": "1.21.1",
                "mainClass": "net.minecraft.client.main.Main",
                "arguments": {"game": ["--parentGame"], "jvm": ["--parentJvm"]},
                "assetIndex": {"id": "17", "sha1": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "size": 1, "url": "https://piston-meta.mojang.com/x"},
                "downloads": {"client": {"sha1": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "size": 2, "url": "https://piston-data.mojang.com/x"}},
                "libraries": [{"name": "parent:lib:1"}]
            }"#,
        )
        .unwrap();
        let child: VersionJson = serde_json::from_str(
            r#"{
                "id": "neoforge-21.1.248",
                "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher",
                "inheritsFrom": "1.21.1",
                "arguments": {"game": ["--childGame"], "jvm": ["--childJvm"]},
                "libraries": [{"name": "child:lib:1"}]
            }"#,
        )
        .unwrap();

        let merged = merge_versions(&parent, Some(&child)).unwrap();
        assert_eq!(merged.id, "neoforge-21.1.248");
        assert_eq!(merged.client_jar_version_id, "1.21.1");
        assert_eq!(merged.main_class, "cpw.mods.bootstraplauncher.BootstrapLauncher");
        assert_eq!(resolve_arguments(&merged.game_arguments, &HashMap::new()), vec!["--parentGame", "--childGame"]);
        assert_eq!(resolve_arguments(&merged.jvm_arguments, &HashMap::new()), vec!["--parentJvm", "--childJvm"]);
        assert_eq!(merged.libraries.iter().map(|library| library.name.as_str()).collect::<Vec<_>>(), vec!["parent:lib:1", "child:lib:1"]);
        assert_eq!(merged.asset_index.id, "17");
    }

    #[test]
    fn disallowed_host_is_rejected() {
        assert!(!is_allowed_host("https://example.com/evil.jar"));
        assert!(is_allowed_host("https://piston-data.mojang.com/v1/objects/x/client.jar"));
    }

    /// Live smoke test against the real Mojang CDN: manifest -> version JSON
    /// (SHA-1 verified) -> asset index -> a handful of libraries + the
    /// client jar. Not run by default (`cargo test`); run explicitly with
    /// `cargo test -- --ignored mojang::` when checking real connectivity.
    #[test]
    #[ignore]
    fn live_fetches_1_21_1_and_downloads_a_few_files() {
        let client = Client::builder().build().unwrap();
        let manifest = fetch_version_manifest(&client).unwrap();
        let entry = find_version(&manifest, "1.21.1").expect("1.21.1 must be listed");
        let version = fetch_version_json(&client, entry).unwrap();
        assert_eq!(version.main_class, "net.minecraft.client.main.Main");

        let game_dir = std::env::temp_dir().join(format!("shacraft-mojang-live-{}", std::process::id()));

        let merged = merge_versions(&version, None).unwrap();
        let client_jar = ensure_client_jar(&client, &game_dir, &merged.id, &merged.client).unwrap();
        assert!(client_jar.exists());

        let asset_index = ensure_asset_index(&client, &game_dir, &merged.asset_index).unwrap();
        assert!(!asset_index.objects.is_empty());

        let mut small_libraries: Vec<Library> = merged
            .libraries
            .iter()
            .filter(|library| library.downloads.as_ref().and_then(|downloads| downloads.artifact.as_ref()).is_some_and(|artifact| artifact.size < 200_000))
            .take(5)
            .cloned()
            .collect();
        assert!(!small_libraries.is_empty(), "expected at least one small library to sanity-check downloads with");
        small_libraries.truncate(5);
        let progress: ProgressCallback = Arc::new(|_, _| {});
        let paths = ensure_libraries(&client, &game_dir, &small_libraries, &progress).unwrap();
        for path in &paths {
            assert!(path.exists(), "{path:?} should have been downloaded");
        }

        // Re-running against already-downloaded files must be a no-op (the
        // `is_current` fast path), not re-download or fail.
        let client_jar_again = ensure_client_jar(&client, &game_dir, &merged.id, &merged.client).unwrap();
        assert_eq!(client_jar, client_jar_again);

        fs::remove_dir_all(&game_dir).ok();
    }
}
