use super::data_dir;
use crate::{operations::LauncherOperations, profile, remote};
use tauri::{AppHandle, State};

/// Loads and validates the published ShaCraft manifest before inspecting a profile.
#[tauri::command]
pub(crate) async fn inspect_remote_profile(
    app: AppHandle,
    profile_id: String,
) -> Result<profile::ProfileInspection, String> {
    let data_dir = data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let manifest = remote::fetch_manifest(&profile_id).map_err(|error| error.to_string())?;
        profile::inspect(&data_dir.join("profiles").join(&manifest.id), &manifest)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Profile inspection task failed: {error}"))?
}

/// Downloads missing or changed ShaCraft-managed files from the fixed v2 endpoint.
#[tauri::command]
pub(crate) async fn sync_remote_profile(
    app: AppHandle,
    state: State<'_, LauncherOperations>,
    profile_id: String,
) -> Result<profile::SyncResult, String> {
    let data_dir = data_dir(&app)?;
    let permit = state.installation.acquire("Installation")?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let manifest = remote::fetch_manifest(&profile_id).map_err(|error| error.to_string())?;
        profile::sync(&data_dir.join("profiles").join(&manifest.id), &manifest)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Profile synchronization task failed: {error}"))?
}
