use super::data_dir;
use crate::settings;
use tauri::AppHandle;

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
    settings: settings::LauncherSettings,
) -> Result<settings::LauncherSettings, String> {
    let data_dir = data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || settings::save(&data_dir, settings))
        .await
        .map_err(|error| format!("Settings task failed: {error}"))?
        .map_err(|error| error.to_string())
}
