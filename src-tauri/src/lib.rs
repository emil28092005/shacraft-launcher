mod java;
mod manifest;
mod profile;
mod remote;
mod settings;

use serde::Serialize;
use tauri::{AppHandle, Manager};

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
            save_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running ShaCraft Launcher");
}
