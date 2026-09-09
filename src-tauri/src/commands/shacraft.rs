//! ShaCraft sessions and verified account links are the launch identity source.
use super::data_dir;
use crate::{operations::LauncherOperations, shacraft_account};
use std::path::Path;
use tauri::{AppHandle, State};

async fn account_task<T: Send + 'static>(
    app: &AppHandle,
    operations: &LauncherOperations,
    work: impl FnOnce(&Path) -> Result<T, shacraft_account::AccountError> + Send + 'static,
) -> Result<T, String> {
    let directory = data_dir(app)?;
    let permit = operations
        .shacraft_account
        .acquire("ShaCraft account operation")?;
    tauri::async_runtime::spawn_blocking(move || {
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
pub(crate) async fn shacraft_claim_nickname(
    app: AppHandle,
    state: State<'_, LauncherOperations>,
    nickname: String,
) -> Result<shacraft_account::Account, String> {
    account_task(&app, &state, move |directory| {
        shacraft_account::claim_nickname(directory, &nickname)
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
