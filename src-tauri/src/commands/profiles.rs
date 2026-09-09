use super::data_dir;
use crate::{installation_lock::InstallationLock, operations::LauncherOperations, profile, remote};
use serde::Serialize;
use tauri::{AppHandle, State};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProfileMetadata {
    pub snapshot: String,
    pub minecraft_version: String,
    pub loader_kind: String,
    pub loader_version: String,
    pub java_major: u8,
}
impl From<&remote::VerifiedSnapshot> for ProfileMetadata {
    fn from(snapshot: &remote::VerifiedSnapshot) -> Self {
        let game = &snapshot.manifest.minecraft;
        Self {
            snapshot: snapshot.digest.clone(),
            minecraft_version: game.version.clone(),
            loader_kind: game.loader.kind.clone(),
            loader_version: game.loader.version.clone(),
            java_major: game.java_major,
        }
    }
}

#[tauri::command]
pub(crate) async fn profile_metadata(profile_id: String) -> Result<ProfileMetadata, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let snapshot = remote::fetch_snapshot(&profile_id).map_err(|e| e.to_string())?;
        Ok(ProfileMetadata::from(&snapshot))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn get_server_status(profile_id: String) -> Result<remote::ServerStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        remote::fetch_server_status(&profile_id).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn inspect_remote_profile(
    app: AppHandle,
    profile_id: String,
) -> Result<profile::ProfileInspection, String> {
    let directory = data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        // Inspection must not report a partially applied journal as ready.
        let _lock = InstallationLock::acquire(&directory)?;
        let manifest = remote::fetch_manifest(&profile_id).map_err(|e| e.to_string())?;
        profile::inspect(&directory.join("profiles").join(&manifest.id), &manifest)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn sync_remote_profile(
    app: AppHandle,
    state: State<'_, LauncherOperations>,
    profile_id: String,
) -> Result<profile::SyncResult, String> {
    let directory = data_dir(&app)?;
    let permit = state.installation.acquire("Installation")?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let _lock = InstallationLock::acquire(&directory)?;
        let snapshot = remote::fetch_snapshot(&profile_id).map_err(|e| e.to_string())?;
        profile::sync_snapshot(
            &directory.join("profiles").join(&snapshot.manifest.id),
            &snapshot,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn legacy_mods(
    app: AppHandle,
    profile_id: String,
) -> Result<Vec<profile::LegacyMod>, String> {
    let directory = data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _lock = InstallationLock::acquire(&directory)?;
        let manifest = remote::fetch_manifest(&profile_id).map_err(|e| e.to_string())?;
        profile::list_legacy_mods(&directory.join("profiles").join(&manifest.id), &manifest)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn backup_legacy_mods(
    app: AppHandle,
    state: State<'_, LauncherOperations>,
    profile_id: String,
    selections: Vec<profile::LegacySelection>,
) -> Result<profile::LegacyBackup, String> {
    let directory = data_dir(&app)?;
    let permit = state.installation.acquire("Legacy migration")?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let _lock = InstallationLock::acquire(&directory)?;
        let manifest = remote::fetch_manifest(&profile_id).map_err(|e| e.to_string())?;
        profile::backup_legacy_mods(
            &directory.join("profiles").join(&manifest.id),
            &manifest,
            &selections,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
