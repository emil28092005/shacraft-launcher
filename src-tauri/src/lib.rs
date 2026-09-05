mod manifest;

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

/// Validates an untrusted profile manifest before any file is downloaded.
#[tauri::command]
fn validate_manifest(manifest_json: String) -> Result<(), String> {
    manifest::validate_json(&manifest_json).map(|_| ()).map_err(|error| error.to_string())
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![native_host, validate_manifest])
        .run(tauri::generate_context!())
        .expect("error while running ShaCraft Launcher");
}
