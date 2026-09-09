//! ShaCraft sessions and verified account links are the launch identity source.
use super::data_dir;
use crate::{
    operations::{LauncherOperations, Operation},
    session, shacraft_account,
};
use std::path::Path;
use tauri::{AppHandle, State};

async fn account_task<T: Send + 'static>(
    app: &AppHandle,
    operations: &LauncherOperations,
    work: impl FnOnce(&Path) -> Result<T, shacraft_account::AccountError> + Send + 'static,
) -> Result<T, String> {
    let directory = data_dir(app)?;
    let lifecycle = crate::update_guard::begin_operation(&directory, operations)?;
    let permit = operations
        .shacraft_account
        .acquire("ShaCraft account operation")?;
    tauri::async_runtime::spawn_blocking(move || {
        let _lifecycle = lifecycle;
        let _permit = permit;
        work(&directory).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("ShaCraft account task failed: {error}"))?
}

#[tauri::command]
pub(crate) async fn shacraft_authenticate(
    app: AppHandle,
    state: State<'_, LauncherOperations>,
    username: String,
    password: String,
    register: bool,
) -> Result<shacraft_account::LoginResult, String> {
    account_task(&app, &state, move |directory| {
        shacraft_account::authenticate(directory, &username, &password, register)
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_shacraft_account(
    app: AppHandle,
    state: State<'_, LauncherOperations>,
) -> Result<Option<shacraft_account::Account>, String> {
    account_task(
        &app,
        &state,
        |directory| match shacraft_account::get_account(directory) {
            Ok(account) => Ok(Some(account)),
            Err(shacraft_account::AccountError::InvalidSession) => Ok(None),
            Err(error) => Err(error),
        },
    )
    .await
}

#[tauri::command]
pub(crate) async fn shacraft_logout(
    app: AppHandle,
    state: State<'_, LauncherOperations>,
) -> Result<(), String> {
    account_task(&app, &state, shacraft_account::logout).await
}

#[tauri::command]
pub(crate) async fn shacraft_start_link(
    app: AppHandle,
    state: State<'_, LauncherOperations>,
    nickname: String,
) -> Result<shacraft_account::LinkStart, String> {
    account_task(&app, &state, move |directory| {
        shacraft_account::start_link(directory, "aoc", &nickname)
    })
    .await
}

#[tauri::command]
pub(crate) async fn shacraft_link_status(
    app: AppHandle,
    state: State<'_, LauncherOperations>,
    challenge_id: i64,
) -> Result<shacraft_account::LinkStatus, String> {
    account_task(&app, &state, move |directory| {
        shacraft_account::link_status(directory, challenge_id)
    })
    .await
}

/// Always revalidate the server session and its aoc link. Legacy local settings
/// and Microsoft tokens do not select the identity in the ShaCraft-only flow.
pub(super) fn resolve_identity(
    directory: &Path,
    operation: &Operation,
) -> Result<session::PlayerIdentity, String> {
    let _permit = operation.acquire("ShaCraft account operation")?;
    let name =
        shacraft_account::aeronautics_nickname(directory).map_err(|error| error.to_string())?;
    Ok(session::PlayerIdentity::Offline { name })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_shacraft_session_cannot_fall_back_to_legacy_nickname() {
        let directory = std::env::temp_dir().join(format!(
            "shacraft-identity-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        crate::settings::save(&directory, crate::settings::LauncherSettings::default()).unwrap();
        assert!(resolve_identity(&directory, &Operation::default()).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
