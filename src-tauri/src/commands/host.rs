use super::data_dir;
use crate::{java, manifest};
use serde::Serialize;
use tauri::AppHandle;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeHost {
    platform: &'static str,
    data_dir: String,
    launcher_version: &'static str,
}

/// Returns non-sensitive environment information needed by the interface.
#[tauri::command]
pub(crate) fn native_host(app: AppHandle) -> Result<NativeHost, String> {
    let data_dir = data_dir(&app)?;

    Ok(NativeHost {
        platform: std::env::consts::OS,
        data_dir: data_dir.display().to_string(),
        launcher_version: env!("CARGO_PKG_VERSION"),
    })
}

/// Detects an existing Java installation. This is read-only and never downloads Java.
#[tauri::command]
pub(crate) fn detect_java() -> Option<java::JavaInstallation> {
    java::detect()
}

/// Validates an untrusted profile manifest before any file is downloaded.
#[tauri::command]
pub(crate) fn validate_manifest(manifest_json: String) -> Result<(), String> {
    manifest::validate_json(&manifest_json)
        .map(|_| ())
        .map_err(|error| error.to_string())
}
