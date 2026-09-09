use super::data_dir;
use crate::{operations::LauncherOperations, settings};
use tauri::{AppHandle, State};

#[tauri::command]
pub(crate) async fn load_settings(app: AppHandle) -> Result<settings::LauncherSettings, String> {
    let data_dir = data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || settings::load(&data_dir))
        .await
        .map_err(|error| format!("Settings task failed: {error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn save_settings(
    app: AppHandle,
    state: State<'_, LauncherOperations>,
    settings: settings::LauncherSettings,
) -> Result<settings::LauncherSettings, String> {
    let data_dir = data_dir(&app)?;
    let lifecycle = crate::update_guard::begin_operation(&data_dir, &state)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _lifecycle = lifecycle;
        settings::save(&data_dir, settings)
    })
    .await
    .map_err(|error| format!("Settings task failed: {error}"))?
    .map_err(|error| error.to_string())
}
