//! NeoForge trust boundary: downloads the official installer for
//! `manifest.minecraft.loader.version` from `maven.neoforged.net` and runs
//! it headlessly to produce a standard, vanilla-launcher-compatible version
//! profile under the shared game directory.
//!
//! We deliberately do not reimplement the installer's client processor
//! pipeline (mapping extraction, jar splitting, renaming, binary patching):
//! running NeoForge's own official installer jar is far less code, matches
//! exactly what a human running the installer manually would get, and
//! survives future NeoForge releases changing their processor format.
//!
//! Empirically verified (2026-09-06, against the real
//! neoforge-21.1.248-installer.jar and a real Temurin 21 JRE): the
//! installer's `net.minecraftforge.installer.SimpleInstaller` refuses to
//! target a directory that doesn't already look like a `.minecraft` folder
//! ("you need to run the launcher first!") unless a `launcher_profiles.json`
//! stub already exists there — see `ensure_launcher_profiles_stub`. After
//! that, `--installClient <dir>` downloads/patches everything itself and
//! writes a standard `versions/neoforge-<version>/neoforge-<version>.json`
//! that inherits from the vanilla version and needs no NeoForge-specific
//! classpath handling: `mojang::merge_versions` + the resulting libraries
//! list is everything `launch.rs` needs. The separately-produced
//! `libraries/net/neoforged/neoforge/<version>/neoforge-<version>-client.jar`
//! is loaded by FancyModLoader itself at runtime (via the `--fml.*` game
//! arguments already present on the merged profile) and is intentionally
//! never added to our own classpath.

use crate::download::{self, Checksum, DownloadError, ProgressCallback};
use crate::mojang::VersionJson;
use reqwest::blocking::Client;
use std::{
    fmt, fs, io,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
};
use url::Url;

const NEOFORGE_HOST: &str = "maven.neoforged.net";

/// Minimal `launcher_profiles.json` accepted by the legacy NeoForge/Forge
/// installer as proof that a directory is a legitimate launcher data
/// directory. Written once; never overwrites an existing file.
const LAUNCHER_PROFILES_STUB: &str = r#"{"profiles":{},"selectedProfile":"","clientToken":"","authenticationDatabase":{},"settings":{"enableSnapshots":false,"enableAdvanced":false,"keepLauncherOpen":false,"soundOn":false,"showGameLog":false,"profileSorting":"ByLastPlayed","showMenu":false,"enableHistorical":false,"enableReleases":true,"crashAssistance":true},"version":3}"#;

#[derive(Debug)]
pub enum NeoForgeError {
    DisallowedHost(String),
    Network(reqwest::Error),
    HttpStatus(reqwest::StatusCode),
    InvalidChecksum(String),
    Download(DownloadError),
    Io(io::Error),
    InvalidJson(serde_json::Error),
    InstallerFailed {
        exit_code: Option<i32>,
        output_tail: String,
    },
}

impl fmt::Display for NeoForgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DisallowedHost(url) => {
                write!(formatter, "URL is not a recognised NeoForge host: {url}")
            }
            Self::Network(error) => write!(formatter, "network error: {error}"),
            Self::HttpStatus(status) => write!(formatter, "maven.neoforged.net returned {status}"),
            Self::InvalidChecksum(text) => {
                write!(formatter, "unexpected checksum response: {text}")
            }
            Self::Download(error) => write!(formatter, "{error}"),
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::InvalidJson(error) => write!(formatter, "invalid NeoForge version JSON: {error}"),
            Self::InstallerFailed {
                exit_code,
                output_tail,
            } => {
                write!(
                    formatter,
                    "NeoForge installer failed (exit {exit_code:?}):\n{output_tail}"
                )
            }
        }
    }
}

impl From<DownloadError> for NeoForgeError {
    fn from(error: DownloadError) -> Self {
        Self::Download(error)
    }
}
impl From<io::Error> for NeoForgeError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub(crate) fn is_allowed_host(url: &str) -> bool {
    Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(|host| host == NEOFORGE_HOST))
        .unwrap_or(false)
}

fn installer_jar_url(loader_version: &str) -> String {
    format!("https://{NEOFORGE_HOST}/releases/net/neoforged/neoforge/{loader_version}/neoforge-{loader_version}-installer.jar")
}

/// Downloads (or reuses a cached, still-valid) NeoForge installer jar,
/// verified against the `.sha256` sidecar Maven publishes next to every
/// artifact.
pub fn ensure_installer(
    client: &Client,
    cache_dir: &Path,
    loader_version: &str,
) -> Result<PathBuf, NeoForgeError> {
    let jar_url = installer_jar_url(loader_version);
    let checksum_url = format!("{jar_url}.sha256");
    if !is_allowed_host(&jar_url) {
        return Err(NeoForgeError::DisallowedHost(jar_url));
    }

    let response = client
        .get(&checksum_url)
        .send()
        .map_err(NeoForgeError::Network)?;
    if !response.status().is_success() {
        return Err(NeoForgeError::HttpStatus(response.status()));
    }
    let sha256 = response
        .text()
        .map_err(NeoForgeError::Network)?
        .trim()
        .to_ascii_lowercase();
    if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(NeoForgeError::InvalidChecksum(sha256));
    }

    let target = cache_dir.join(format!("neoforge-{loader_version}-installer.jar"));
    let checksum = Checksum::Sha256(sha256);
    if !download::is_current(&target, None, &checksum)? {
        download::download_verified(client, &jar_url, &target, None, &checksum, |_, _| {})?;
    }
    Ok(target)
}

fn ensure_launcher_profiles_stub(game_dir: &Path) -> io::Result<()> {
    let path = game_dir.join("launcher_profiles.json");
    if path.exists() {
        return Ok(());
    }
    fs::create_dir_all(game_dir)?;
    fs::write(path, LAUNCHER_PROFILES_STUB)
}

pub fn installed_version_json_path(game_dir: &Path, loader_version: &str) -> PathBuf {
    game_dir
        .join("versions")
        .join(format!("neoforge-{loader_version}"))
        .join(format!("neoforge-{loader_version}.json"))
}

fn patched_client_path(game_dir: &Path, loader_version: &str) -> PathBuf {
    game_dir
        .join("libraries/net/neoforged/neoforge")
        .join(loader_version)
        .join(format!("neoforge-{loader_version}-client.jar"))
}

fn is_nonempty_file(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
}

fn installation_complete(game_dir: &Path, loader_version: &str) -> bool {
    is_nonempty_file(&installed_version_json_path(game_dir, loader_version))
        && is_nonempty_file(&patched_client_path(game_dir, loader_version))
}

/// The installer jar bundles its own `install_profile.json`, which lists
/// exactly which libraries it will download and which processors it will
/// run to patch the client — the same manifest the installer itself reads.
/// Reading it upfront gives a real, version-agnostic total for progress
/// reporting instead of a guessed constant.
fn read_install_profile_counts(installer_path: &Path) -> Option<(u64, u64)> {
    let file = fs::File::open(installer_path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let mut entry = archive.by_name("install_profile.json").ok()?;
    let mut contents = String::new();
    entry.read_to_string(&mut contents).ok()?;
    let profile: serde_json::Value = serde_json::from_str(&contents).ok()?;
    let libraries = profile.get("libraries")?.as_array()?.len() as u64;
    let processors = profile.get("processors")?.as_array()?.len() as u64;
    Some((libraries, processors))
}

/// Bumps `downloads_done`/`processors_done` from one line of the installer's
/// output and reports the combined total, clamped so a miscount (e.g. the
/// installer logging a couple of extra non-library downloads) never exceeds
/// or exceeds `total` by much. `total_libraries` caps the download half so
/// those extra lines cannot crowd out the processor half of the bar.
fn observe_installer_line(
    line: &str,
    downloads_done: &AtomicU64,
    processors_done: &AtomicU64,
    total_libraries: u64,
    total: u64,
    on_progress: &ProgressCallback,
) {
    let trimmed = line.trim_start();
    if trimmed.starts_with("Download completed") {
        downloads_done.fetch_add(1, Ordering::Relaxed);
    } else if trimmed.starts_with("Processor: ") && trimmed.matches(':').count() == 2 {
        // Exactly two colons is the processor *header* line
        // ("Processor: net.neoforged.installertools:jarsplitter"); its
        // sub-step lines ("Processor: ...: Loading patch files") have three.
        processors_done.fetch_add(1, Ordering::Relaxed);
    } else {
        return;
    }
    let current = downloads_done.load(Ordering::Relaxed).min(total_libraries)
        + processors_done.load(Ordering::Relaxed);
    on_progress(current.min(total), total);
}

fn truncate_tail(text: &str) -> String {
    text.chars()
        .rev()
        .take(4000)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

/// Runs the installer with piped output, reporting live progress as its own
/// log lines confirm each library download and processor step, instead of
/// blocking silently until the whole (often minutes-long) run finishes.
/// Returns the process's exit code and its combined stdout+stderr, which the
/// caller uses to build a diagnostic if the install turns out to have failed
/// silently (exit 0 but no version JSON produced).
fn run_installer_with_progress(
    java_executable: &Path,
    installer_path: &Path,
    game_dir: &Path,
    cache_dir: &Path,
    total_libraries: u64,
    total: u64,
    on_progress: &ProgressCallback,
) -> Result<(Option<i32>, String), NeoForgeError> {
    let mut child = Command::new(java_executable)
        .arg("-jar")
        .arg(installer_path)
        .arg("--installClient")
        .arg(game_dir)
        .current_dir(cache_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    let combined_log = Arc::new(Mutex::new(String::new()));
    let downloads_done = Arc::new(AtomicU64::new(0));
    let processors_done = Arc::new(AtomicU64::new(0));

    let stdout_thread = {
        let combined_log = Arc::clone(&combined_log);
        let downloads_done = Arc::clone(&downloads_done);
        let processors_done = Arc::clone(&processors_done);
        let on_progress = Arc::clone(on_progress);
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                observe_installer_line(
                    &line,
                    &downloads_done,
                    &processors_done,
                    total_libraries,
                    total,
                    &on_progress,
                );
                let mut log = combined_log.lock().unwrap();
                log.push_str(&line);
                log.push('\n');
            }
        })
    };
    let stderr_thread = {
        let combined_log = Arc::clone(&combined_log);
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let mut log = combined_log.lock().unwrap();
                log.push_str(&line);
                log.push('\n');
            }
        })
    };

    let status = child.wait()?;
    stdout_thread.join().ok();
    stderr_thread.join().ok();
    let tail = truncate_tail(&combined_log.lock().unwrap());

    if !status.success() {
        return Err(NeoForgeError::InstallerFailed {
            exit_code: status.code(),
            output_tail: tail,
        });
    }
    Ok((status.code(), tail))
}

/// Ensures NeoForge `loader_version` is installed into the shared
/// `game_dir` (vanilla libraries/version must already be there so the
/// installer can reuse them). No-op if already installed. Runs the
/// installer headlessly with `java_executable`; its own network calls go
/// straight to `maven.neoforged.net`/Mojang, outside our control, which is
/// an accepted trust delegation to NeoForge's official tooling once the
/// installer binary itself is SHA-256 verified. `on_progress` reports real
/// progress (installer-confirmed library downloads plus patch-processor
/// steps, read from the installer's own `install_profile.json`) while it
/// runs; it fires once with `(1, 1)` when already installed.
pub fn ensure_client_installed(
    client: &Client,
    java_executable: &Path,
    game_dir: &Path,
    cache_dir: &Path,
    loader_version: &str,
    on_progress: &ProgressCallback,
) -> Result<VersionJson, NeoForgeError> {
    let version_json_path = installed_version_json_path(game_dir, loader_version);
    if !installation_complete(game_dir, loader_version) {
        ensure_launcher_profiles_stub(game_dir)?;
        let installer_path = ensure_installer(client, cache_dir, loader_version)?;

        // A leftover version JSON makes some installer versions treat the
        // profile as already installed even when the patched client was
        // deleted or quarantined. Remove only that generated marker so the
        // official installer is forced to rebuild the incomplete profile.
        match fs::remove_file(&version_json_path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(NeoForgeError::Io(error)),
        }

        let (total_libraries, total_processors) =
            read_install_profile_counts(&installer_path).unwrap_or((0, 0));
        let total = (total_libraries + total_processors).max(1);
        on_progress(0, total);

        let (exit_code, tail) = run_installer_with_progress(
            java_executable,
            &installer_path,
            game_dir,
            cache_dir,
            total_libraries,
            total,
            on_progress,
        )?;
        if !installation_complete(game_dir, loader_version) {
            return Err(NeoForgeError::InstallerFailed {
                exit_code,
                output_tail: tail,
            });
        }
        on_progress(total, total);
    } else {
        on_progress(1, 1);
    }

    let bytes = fs::read(&version_json_path)?;
    serde_json::from_slice(&bytes).map_err(NeoForgeError::InvalidJson)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installer_url_matches_maven_layout() {
        assert_eq!(
            installer_jar_url("21.1.248"),
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.248/neoforge-21.1.248-installer.jar"
        );
    }

    #[test]
    fn rejects_non_neoforge_hosts() {
        assert!(!is_allowed_host("https://example.com/evil.jar"));
        assert!(is_allowed_host(
            "https://maven.neoforged.net/releases/x.jar"
        ));
    }

    #[test]
    fn launcher_profiles_stub_is_idempotent() {
        let dir =
            std::env::temp_dir().join(format!("shacraft-neoforge-test-{}", std::process::id()));
        ensure_launcher_profiles_stub(&dir).unwrap();
        let first = fs::read_to_string(dir.join("launcher_profiles.json")).unwrap();
        fs::write(dir.join("launcher_profiles.json"), "custom-content").unwrap();
        ensure_launcher_profiles_stub(&dir).unwrap();
        let second = fs::read_to_string(dir.join("launcher_profiles.json")).unwrap();
        assert_eq!(second, "custom-content");
        assert!(first.contains("\"profiles\""));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn incomplete_install_is_not_accepted() {
        let dir = std::env::temp_dir().join(format!(
            "shacraft-neoforge-completeness-test-{}",
            std::process::id()
        ));
        let version = "21.1.248";
        let json = installed_version_json_path(&dir, version);
        fs::create_dir_all(json.parent().unwrap()).unwrap();
        fs::write(&json, b"{}").unwrap();
        assert!(!installation_complete(&dir, version));

        let client = patched_client_path(&dir, version);
        fs::create_dir_all(client.parent().unwrap()).unwrap();
        fs::write(&client, b"patched").unwrap();
        assert!(installation_complete(&dir, version));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn observe_installer_line_counts_downloads_and_processor_headers() {
        let downloads_done = AtomicU64::new(0);
        let processors_done = AtomicU64::new(0);
        let calls: Arc<Mutex<Vec<(u64, u64)>>> = Arc::new(Mutex::new(Vec::new()));
        let on_progress: ProgressCallback = {
            let calls = Arc::clone(&calls);
            Arc::new(move |current, total| calls.lock().unwrap().push((current, total)))
        };
        let total_libraries = 2;
        let total = 3; // 2 libraries + 1 processor

        // A "Downloading library from ..." start line reports nothing by
        // itself; only its "Download completed" confirmation counts.
        observe_installer_line(
            "Downloading library from https://example/a.jar",
            &downloads_done,
            &processors_done,
            total_libraries,
            total,
            &on_progress,
        );
        observe_installer_line(
            "Download completed: Checksum validated.",
            &downloads_done,
            &processors_done,
            total_libraries,
            total,
            &on_progress,
        );
        observe_installer_line(
            "Download completed: Checksum validated.",
            &downloads_done,
            &processors_done,
            total_libraries,
            total,
            &on_progress,
        );
        observe_installer_line(
            "Processor: net.neoforged.installertools:jarsplitter",
            &downloads_done,
            &processors_done,
            total_libraries,
            total,
            &on_progress,
        );
        // A processor's sub-step lines (three colons) must not double-count.
        observe_installer_line(
            "Processor: net.neoforged.installertools:jarsplitter: Loading patch files",
            &downloads_done,
            &processors_done,
            total_libraries,
            total,
            &on_progress,
        );

        assert_eq!(*calls.lock().unwrap(), vec![(1, 3), (2, 3), (3, 3)]);
    }

    /// Full live pipeline: provisions a real Java 21 (runtime.rs) if none
    /// is already usable, then runs the real NeoForge 21.1.248 installer
    /// into an empty game dir (it fetches and patches vanilla 1.21.1
    /// itself — confirmed manually, no pre-seeding needed) and checks the
    /// installed profile merges into a launch-shaped spec together with a
    /// separately-fetched vanilla version JSON (mojang.rs), exactly as
    /// `lib.rs`'s `ensure_game_installed` command will do it. Not run by
    /// default; `cargo test -- --ignored live_full_pipeline`.
    #[test]
    #[ignore]
    fn live_full_pipeline_installs_neoforge() {
        use crate::{java, mojang};

        let client = Client::builder().build().unwrap();
        let root =
            std::env::temp_dir().join(format!("shacraft-neoforge-pipeline-{}", std::process::id()));
        let game_dir = root.join("game");
        let cache_dir = root.join("cache");
        fs::create_dir_all(&cache_dir).unwrap();

        let manifest = mojang::fetch_version_manifest(&client).unwrap();
        let entry = mojang::find_version(&manifest, "1.21.1").unwrap();
        let vanilla = mojang::fetch_version_json(&client, entry).unwrap();

        let no_progress: ProgressCallback = Arc::new(|_, _| {});
        let java_install =
            java::ensure_java(&client, &root.join("runtime"), 21, &no_progress).unwrap();

        // The installer fetches and patches vanilla itself; we don't
        // pre-download it. It only needs a Java runtime and an empty dir.
        let progress_calls: Arc<Mutex<Vec<(u64, u64)>>> = Arc::new(Mutex::new(Vec::new()));
        let progress: ProgressCallback = {
            let progress_calls = Arc::clone(&progress_calls);
            Arc::new(move |current, total| progress_calls.lock().unwrap().push((current, total)))
        };
        let neoforge_version = ensure_client_installed(
            &client,
            Path::new(&java_install.executable),
            &game_dir,
            &cache_dir,
            "21.1.248",
            &progress,
        )
        .unwrap();
        let merged = mojang::merge_versions(&vanilla, Some(&neoforge_version)).unwrap();
        assert_eq!(
            merged.main_class,
            "cpw.mods.bootstraplauncher.BootstrapLauncher"
        );
        assert!(
            merged.libraries.len() > 100,
            "expected vanilla (97) + neoforge (47) libraries, got {}",
            merged.libraries.len()
        );

        let patched_client =
            game_dir.join("libraries/net/neoforged/neoforge/21.1.248/neoforge-21.1.248-client.jar");
        assert!(
            patched_client.exists(),
            "FancyModLoader needs this at runtime even though it is not on the generic classpath"
        );

        let calls = progress_calls.lock().unwrap();
        assert!(
            calls.len() > 5,
            "expected many incremental progress calls, got {}",
            calls.len()
        );
        let (last_current, last_total) = *calls.last().unwrap();
        assert_eq!(
            last_current, last_total,
            "progress must reach 100% on success"
        );
        assert!(
            calls.windows(2).all(|pair| pair[0].0 <= pair[1].0),
            "reported progress must never go backwards"
        );
        drop(calls);

        // Re-running must skip straight to reading the cached version JSON
        // rather than invoking the installer again.
        let neoforge_again = ensure_client_installed(
            &client,
            Path::new(&java_install.executable),
            &game_dir,
            &cache_dir,
            "21.1.248",
            &no_progress,
        )
        .unwrap();
        assert_eq!(
            neoforge_again.libraries.len(),
            neoforge_version.libraries.len()
        );

        fs::remove_dir_all(&root).ok();
    }
}
