mod download;
mod java;
mod launch;
mod manifest;
mod mojang;
mod msa;
mod neoforge;
mod profile;
mod remote;
mod runtime;
mod session;
mod settings;

use reqwest::blocking::Client;
use serde::Serialize;
use std::{path::Path, sync::Arc, time::SystemTime};
use tauri::{AppHandle, Emitter, Manager};

fn http_client() -> Client {
    Client::new()
}

fn game_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|error| format!("Cannot resolve launcher data directory: {error}"))?.join("game"))
}

fn profile_dir(app: &AppHandle, profile_id: &str) -> Result<std::path::PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|error| format!("Cannot resolve launcher data directory: {error}"))?.join("profiles").join(profile_id))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeHost {
    platform: &'static str,
    data_dir: String,
    launcher_version: &'static str,
}

/// Returns non-sensitive environment information needed by the interface.
/// File access and child-process launching are deliberately not exposed yet.
#[tauri::command]
fn native_host(app: AppHandle) -> Result<NativeHost, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Cannot resolve launcher data directory: {error}"))?;

    Ok(NativeHost {
        platform: std::env::consts::OS,
        data_dir: data_dir.display().to_string(),
        launcher_version: env!("CARGO_PKG_VERSION"),
    })
}

/// Detects an existing Java installation. This is read-only and never downloads Java.
#[tauri::command]
fn detect_java() -> Option<java::JavaInstallation> {
    java::detect()
}

/// Validates an untrusted profile manifest before any file is downloaded.
#[tauri::command]
fn validate_manifest(manifest_json: String) -> Result<(), String> {
    manifest::validate_json(&manifest_json).map(|_| ()).map_err(|error| error.to_string())
}

/// Inspects the local profile without changing player files.
#[tauri::command]
async fn inspect_profile(app: AppHandle, manifest_json: String) -> Result<profile::ProfileInspection, String> {
    let manifest = manifest::validate_json(&manifest_json).map_err(|error| error.to_string())?;
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Cannot resolve launcher data directory: {error}"))?
        .join("profiles")
        .join(&manifest.id);

    tauri::async_runtime::spawn_blocking(move || profile::inspect(&root, &manifest))
        .await
        .map_err(|error| format!("Profile inspection task failed: {error}"))?
        .map_err(|error| error.to_string())
}

/// Synchronizes launcher-managed files after manifest validation.
#[tauri::command]
async fn sync_profile(app: AppHandle, manifest_json: String) -> Result<profile::SyncResult, String> {
    let manifest = manifest::validate_json(&manifest_json).map_err(|error| error.to_string())?;
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Cannot resolve launcher data directory: {error}"))?
        .join("profiles")
        .join(&manifest.id);

    tauri::async_runtime::spawn_blocking(move || profile::sync(&root, &manifest))
        .await
        .map_err(|error| format!("Profile synchronization task failed: {error}"))?
        .map_err(|error| error.to_string())
}

/// Loads and validates the published ShaCraft manifest before inspecting a profile.
#[tauri::command]
async fn inspect_remote_profile(app: AppHandle, profile_id: String) -> Result<profile::ProfileInspection, String> {
    let data_dir = app.path().app_data_dir()
        .map_err(|error| format!("Cannot resolve launcher data directory: {error}"))?;
    tauri::async_runtime::spawn_blocking(move || {
        let manifest = remote::fetch_manifest(&profile_id).map_err(|error| error.to_string())?;
        profile::inspect(&data_dir.join("profiles").join(&manifest.id), &manifest)
            .map_err(|error| error.to_string())
    }).await.map_err(|error| format!("Profile inspection task failed: {error}"))?
}

/// Downloads missing or changed ShaCraft-managed files from the fixed v2 endpoint.
#[tauri::command]
async fn sync_remote_profile(app: AppHandle, profile_id: String) -> Result<profile::SyncResult, String> {
    let data_dir = app.path().app_data_dir()
        .map_err(|error| format!("Cannot resolve launcher data directory: {error}"))?;
    tauri::async_runtime::spawn_blocking(move || {
        let manifest = remote::fetch_manifest(&profile_id).map_err(|error| error.to_string())?;
        profile::sync(&data_dir.join("profiles").join(&manifest.id), &manifest)
            .map_err(|error| error.to_string())
    }).await.map_err(|error| format!("Profile synchronization task failed: {error}"))?
}

#[tauri::command]
async fn load_settings(app: AppHandle) -> Result<settings::LauncherSettings, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Cannot resolve launcher data directory: {error}"))?;
    tauri::async_runtime::spawn_blocking(move || settings::load(&data_dir))
        .await
        .map_err(|error| format!("Settings task failed: {error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn save_settings(app: AppHandle, settings: settings::LauncherSettings) -> Result<settings::LauncherSettings, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Cannot resolve launcher data directory: {error}"))?;
    tauri::async_runtime::spawn_blocking(move || settings::save(&data_dir, settings))
        .await
        .map_err(|error| format!("Settings task failed: {error}"))?
        .map_err(|error| error.to_string())
}

// ---------------------------------------------------------------------
// Microsoft account login
// ---------------------------------------------------------------------

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceCodePayload {
    verification_uri: String,
    user_code: String,
    expires_in_seconds: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LoginResultPayload {
    ok: bool,
    profile: Option<msa::MinecraftProfile>,
    error: Option<String>,
}

/// Starts a Microsoft device-code login in the background. Emits
/// `msa-login-code` as soon as the user code is available (show it to the
/// player immediately — they have a limited time to enter it), then
/// `msa-login-result` once sign-in finishes, fails, or times out. Returns
/// immediately; it does not wait for the user to finish signing in.
#[tauri::command]
fn start_microsoft_login(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let client = http_client();
        let start = match msa::start_device_code(&client) {
            Ok(start) => start,
            Err(error) => {
                let _ = app.emit("msa-login-result", LoginResultPayload { ok: false, profile: None, error: Some(error.to_string()) });
                return;
            }
        };
        let _ = app.emit(
            "msa-login-code",
            DeviceCodePayload { verification_uri: start.verification_uri.clone(), user_code: start.user_code.clone(), expires_in_seconds: start.expires_in_seconds },
        );

        match msa::login_with_device_code(&client, &start) {
            Ok(result) => {
                if let Ok(data_dir) = app.path().app_data_dir() {
                    let _ = msa::save_refresh_token(&data_dir, &result.refresh_token);
                }
                let _ = app.emit("msa-login-result", LoginResultPayload { ok: true, profile: Some(result.profile), error: None });
            }
            Err(error) => {
                let _ = app.emit("msa-login-result", LoginResultPayload { ok: false, profile: None, error: Some(error.to_string()) });
            }
        }
    });
    Ok(())
}

/// Tries to restore a session from a previously saved refresh token
/// (silent, no browser/user code). Returns `None` if there is none saved
/// or it no longer works — the UI should fall back to offering login.
#[tauri::command]
async fn get_account(app: AppHandle) -> Result<Option<msa::MinecraftProfile>, String> {
    let data_dir = app.path().app_data_dir().map_err(|error| format!("Cannot resolve launcher data directory: {error}"))?;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(refresh_token) = msa::load_refresh_token(&data_dir) else { return Ok(None) };
        let client = http_client();
        match msa::login_with_refresh_token(&client, &refresh_token) {
            Ok(result) => {
                let _ = msa::save_refresh_token(&data_dir, &result.refresh_token);
                Ok(Some(result.profile))
            }
            Err(_) => Ok(None),
        }
    })
    .await
    .map_err(|error| format!("Account restore task failed: {error}"))?
}

#[tauri::command]
async fn logout(app: AppHandle) -> Result<(), String> {
    let data_dir = app.path().app_data_dir().map_err(|error| format!("Cannot resolve launcher data directory: {error}"))?;
    tauri::async_runtime::spawn_blocking(move || msa::clear_account(&data_dir))
        .await
        .map_err(|error| format!("Logout task failed: {error}"))?
        .map_err(|error| error.to_string())
}

/// Resolves the identity to launch as, based on the persisted `account_mode`.
/// In `Microsoft` mode this requires a real signed-in session (see
/// `msa::login_with_refresh_token`) and returns an error if there is none;
/// in `Offline` mode it uses the local nickname from settings, so no
/// Microsoft account is needed at all. Offline is never silently used in
/// place of a missing Microsoft session.
fn resolve_identity(client: &Client, data_dir: &Path) -> Result<session::PlayerIdentity, String> {
    let settings = settings::load(data_dir).map_err(|error| error.to_string())?;
    match settings.account_mode {
        settings::AccountMode::Offline => Ok(session::PlayerIdentity::Offline { name: settings.nickname }),
        settings::AccountMode::Microsoft => {
            let refresh_token = msa::load_refresh_token(data_dir).ok_or("Not signed in with a Microsoft account")?;
            let result = msa::login_with_refresh_token(client, &refresh_token).map_err(|error| error.to_string())?;
            let _ = msa::save_refresh_token(data_dir, &result.refresh_token);
            Ok(session::PlayerIdentity::Microsoft(result))
        }
    }
}

// ---------------------------------------------------------------------
// Game install + launch
// ---------------------------------------------------------------------

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallProgress {
    stage: &'static str,
    current_bytes: u64,
    total_bytes: u64,
}

/// Resolves the vanilla + (if any) loader version JSONs for `manifest` and
/// merges them, ensuring a Java runtime and (for NeoForge profiles) running
/// the installer along the way. Shared by `ensure_game_installed` and
/// `launch_game` so both always agree on exactly what "installed" means.
/// `on_progress` is forwarded to the NeoForge installer when one runs;
/// callers that don't display progress (e.g. `launch_game`, which only
/// hits this after `ensure_game_installed` already installed everything)
/// pass a no-op callback.
fn resolve_merged_version(client: &Client, manifest: &manifest::Manifest, java_executable: &Path, game_dir: &Path, cache_dir: &Path, on_progress: &mojang::ProgressCallback) -> Result<mojang::MergedVersion, String> {
    let mojang_manifest = mojang::fetch_version_manifest(client).map_err(|error| error.to_string())?;
    let vanilla_entry = mojang::find_version(&mojang_manifest, &manifest.minecraft.version)
        .ok_or_else(|| format!("Mojang does not list Minecraft version {}", manifest.minecraft.version))?;
    let vanilla = mojang::fetch_version_json(client, vanilla_entry).map_err(|error| error.to_string())?;

    if manifest.minecraft.loader.kind == "neoforge" {
        let neoforge_version = neoforge::ensure_client_installed(client, java_executable, game_dir, cache_dir, &manifest.minecraft.loader.version, on_progress).map_err(|error| error.to_string())?;
        mojang::merge_versions(&vanilla, Some(&neoforge_version)).map_err(|error| error.to_string())
    } else {
        mojang::merge_versions(&vanilla, None).map_err(|error| error.to_string())
    }
}

/// Downloads and installs everything needed to run `profile_id`: the
/// exact Minecraft/loader version the ShaCraft-signed manifest specifies,
/// a Java runtime if none is already usable, and game assets. Emits
/// `game-install-progress` throughout with real progress for every stage:
/// download bytes for Java, installer-confirmed library/processor counts
/// for NeoForge, and download bytes for libraries/assets.
#[tauri::command]
async fn ensure_game_installed(app: AppHandle, profile_id: String) -> Result<(), String> {
    let game_dir = game_dir(&app)?;
    let runtime_root = game_dir.join("runtime");
    let cache_dir = game_dir.join("cache");

    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let client = http_client();
        let manifest = remote::fetch_manifest(&profile_id).map_err(|error| error.to_string())?;

        let stage_progress = |stage: &'static str| -> mojang::ProgressCallback {
            let app = app.clone();
            Arc::new(move |current, total| {
                let _ = app.emit("game-install-progress", InstallProgress { stage, current_bytes: current, total_bytes: total });
            })
        };

        let java_install = java::ensure_java(&client, &runtime_root, manifest.minecraft.java_major, &stage_progress("java")).map_err(|error| error.to_string())?;

        let merged = resolve_merged_version(&client, &manifest, Path::new(&java_install.executable), &game_dir, &cache_dir, &stage_progress("neoforge"))?;
        if manifest.minecraft.loader.kind != "neoforge" {
            // Vanilla-only profiles skip the installer, which normally
            // downloads vanilla itself; do it ourselves here instead.
            mojang::ensure_client_jar(&client, &game_dir, &merged.client_jar_version_id, &merged.client).map_err(|error| error.to_string())?;
            mojang::ensure_libraries(&client, &game_dir, &merged.libraries, &stage_progress("libraries")).map_err(|error| error.to_string())?;
        };

        let asset_index = mojang::ensure_asset_index(&client, &game_dir, &merged.asset_index).map_err(|error| error.to_string())?;
        mojang::ensure_assets(&client, &game_dir, &asset_index, &stage_progress("assets")).map_err(|error| error.to_string())?;

        Ok(())
    })
    .await
    .map_err(|error| format!("Install task failed: {error}"))?
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GameExited {
    profile_id: String,
    exit_code: Option<i32>,
}

/// Launches `profile_id` as the account chosen in settings (`account_mode`).
/// In `Microsoft` mode a real session is required (see `resolve_identity`);
/// in `Offline` mode the local nickname from settings is used, so no
/// Microsoft account is needed. Spawns the game detached; watches it on a
/// background thread only to emit `game-exited` when it eventually closes.
#[tauri::command]
async fn launch_game(app: AppHandle, profile_id: String) -> Result<(), String> {
    let game_dir = game_dir(&app)?;
    let profile_dir = profile_dir(&app, &profile_id)?;
    let data_dir = app.path().app_data_dir().map_err(|error| format!("Cannot resolve launcher data directory: {error}"))?;

    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let client = http_client();
        let identity = resolve_identity(&client, &data_dir)?;
        let manifest = remote::fetch_manifest(&profile_id).map_err(|error| error.to_string())?;
        let settings = settings::load(&data_dir).map_err(|error| error.to_string())?;

        // Everything here should already be installed by `ensure_game_installed`,
        // so these are expected to hit their fast paths; no progress to show.
        let no_progress: mojang::ProgressCallback = Arc::new(|_, _| {});
        let java_install = java::ensure_java(&client, &game_dir.join("runtime"), manifest.minecraft.java_major, &no_progress).map_err(|error| error.to_string())?;
        let merged = resolve_merged_version(&client, &manifest, Path::new(&java_install.executable), &game_dir, &game_dir.join("cache"), &no_progress)?;

        let timestamp = SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        let log_dir = data_dir.join("logs");
        std::fs::create_dir_all(&log_dir).map_err(|error| error.to_string())?;
        let log_path = log_dir.join(format!("{profile_id}-{timestamp}.log"));

        let request = launch::LaunchRequest {
            java_executable: Path::new(&java_install.executable),
            game_dir: &game_dir,
            profile_dir: &profile_dir,
            merged: &merged,
            identity: &identity,
            memory_mb: settings.memory_mb,
            log_path: &log_path,
        };
        let mut child = launch::launch(&request).map_err(|error| error.to_string())?;

        let watch_app = app.clone();
        let watch_profile_id = profile_id.clone();
        std::thread::spawn(move || {
            let exit_code = child.wait().ok().and_then(|status| status.code());
            let _ = watch_app.emit("game-exited", GameExited { profile_id: watch_profile_id, exit_code });
        });

        Ok(())
    })
    .await
    .map_err(|error| format!("Launch task failed: {error}"))?
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            native_host,
            detect_java,
            validate_manifest,
            inspect_profile,
            sync_profile,
            inspect_remote_profile,
            sync_remote_profile,
            load_settings,
            save_settings,
            start_microsoft_login,
            get_account,
            logout,
            ensure_game_installed,
            launch_game
        ])
        .run(tauri::generate_context!())
        .expect("error while running ShaCraft Launcher");
}
