use super::data_dir;
use crate::{
    msa,
    operations::{LauncherOperations, Operation},
    session, settings,
};
use serde::Serialize;
use std::path::Path;
use tauri::{AppHandle, Emitter, State};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceCodePayload {
    verification_uri: String,
    user_code: String,
    expires_in_seconds: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoginResultPayload {
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
pub(crate) fn start_microsoft_login(
    app: AppHandle,
    state: State<'_, LauncherOperations>,
) -> Result<(), String> {
    let directory = data_dir(&app)?;
    let permit = state.account.acquire("Account operation")?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let login = || -> Result<msa::MinecraftProfile, String> {
            let client = msa::http_client().map_err(|error| error.to_string())?;
            let start = msa::start_device_code(&client).map_err(|error| error.to_string())?;
            let _ = app.emit(
                "msa-login-code",
                DeviceCodePayload {
                    verification_uri: start.verification_uri.clone(),
                    user_code: start.user_code.clone(),
                    expires_in_seconds: start.expires_in_seconds,
                },
            );
            let result =
                msa::login_with_device_code(&client, &start).map_err(|error| error.to_string())?;
            msa::save_refresh_token(&directory, &result.refresh_token)
                .map_err(|error| error.to_string())?;
            Ok(result.profile)
        };
        let payload = match login() {
            Ok(profile) => LoginResultPayload {
                ok: true,
                profile: Some(profile),
                error: None,
            },
            Err(error) => LoginResultPayload {
                ok: false,
                profile: None,
                error: Some(error),
            },
        };
        let _ = app.emit("msa-login-result", payload);
    });
    Ok(())
}

/// Tries to restore a session from a previously saved refresh token
/// (silent, no browser/user code). Returns `None` if there is none saved
/// or it no longer works — the UI should fall back to offering login.
#[tauri::command]
pub(crate) async fn get_account(
    app: AppHandle,
    state: State<'_, LauncherOperations>,
) -> Result<Option<msa::MinecraftProfile>, String> {
    let data_dir = data_dir(&app)?;
    let permit = state.account.acquire("Account operation")?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let Some(refresh_token) = msa::load_refresh_token(&data_dir) else {
            return Ok(None);
        };
        let client = msa::http_client().map_err(|error| error.to_string())?;
        match msa::login_with_refresh_token(&client, &refresh_token) {
            Ok(result) => {
                msa::save_refresh_token(&data_dir, &result.refresh_token)
                    .map_err(|error| error.to_string())?;
                Ok(Some(result.profile))
            }
            Err(_) => Ok(None),
        }
    })
    .await
    .map_err(|error| format!("Account restore task failed: {error}"))?
}

#[tauri::command]
pub(crate) async fn logout(
    app: AppHandle,
    state: State<'_, LauncherOperations>,
) -> Result<(), String> {
    let data_dir = data_dir(&app)?;
    let permit = state.account.acquire("Account operation")?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        msa::clear_account(&data_dir)
    })
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
pub(super) fn resolve_identity(
    data_dir: &Path,
    settings: &settings::LauncherSettings,
    account_operation: &Operation,
) -> Result<session::PlayerIdentity, String> {
    match settings.account_mode {
        settings::AccountMode::Offline => Ok(session::PlayerIdentity::Offline {
            name: settings.nickname.clone(),
        }),
        settings::AccountMode::Microsoft => {
            let _permit = account_operation.acquire("Account operation")?;
            let client = msa::http_client().map_err(|error| error.to_string())?;
            let refresh_token = msa::load_refresh_token(data_dir)
                .ok_or("Not signed in with a Microsoft account")?;
            let result = msa::login_with_refresh_token(&client, &refresh_token)
                .map_err(|error| error.to_string())?;
            msa::save_refresh_token(data_dir, &result.refresh_token)
                .map_err(|error| error.to_string())?;
            Ok(session::PlayerIdentity::Microsoft(result))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_identity_does_not_wait_for_microsoft_refresh() {
        let operation = Operation::default();
        let _refresh = operation.acquire("Account operation").unwrap();
        let settings = settings::LauncherSettings::default();
        let identity = resolve_identity(
            Path::new("/unused-account-directory"),
            &settings,
            &operation,
        )
        .unwrap();
        assert!(
            matches!(identity, session::PlayerIdentity::Offline { name } if name == settings.nickname)
        );
    }
}
