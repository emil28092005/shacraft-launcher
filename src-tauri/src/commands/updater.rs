use crate::{
    operations::LauncherOperations,
    updater::{self, LauncherUpdater, Stage, UpdateProgress, UpdateStatus},
};
use tauri::{AppHandle, Emitter, State};

#[tauri::command]
pub(crate) fn get_launcher_update_status(
    app: AppHandle,
    updater: State<'_, LauncherUpdater>,
) -> Result<UpdateStatus, String> {
    updater.status(&app)
}

#[tauri::command]
pub(crate) async fn check_launcher_update(
    app: AppHandle,
    updater: State<'_, LauncherUpdater>,
) -> Result<UpdateStatus, String> {
    let status = updater.status(&app)?;
    if !status.supported || status.stage == Stage::Ready {
        return Ok(status);
    }
    let permit = updater
        .operation
        .acquire("Проверка или установка обновления")
        .map_err(|_| "Проверка или установка обновления уже выполняется.".to_string())?;
    let updater = updater.inner().clone();
    updater.set_stage(Stage::Checking)?;
    // Detached task owns the permit: cancelling an IPC caller cannot release
    // the operation while its native HTTP request is still in flight.
    tauri::async_runtime::spawn(async move {
        let _permit = permit;
        let result = async {
            let key = updater::public_key(&app)?;
            updater::check_candidate(
                updater::trusted_builder(&app)?,
                &key,
                &app.package_info().version.to_string(),
            )
            .await
        }
        .await;
        {
            let mut state = updater
                .state
                .lock()
                .map_err(|_| "Состояние обновления недоступно.".to_string())?;
            match result {
                Ok(candidate) => {
                    state.stage = if candidate.is_some() {
                        Stage::Available
                    } else {
                        Stage::Idle
                    };
                    state.candidate = candidate;
                }
                Err(error) => {
                    state.stage = if state.candidate.is_some() {
                        Stage::Available
                    } else {
                        Stage::Idle
                    };
                    return Err(error);
                }
            }
        }
        updater.status(&app)
    })
    .await
    .map_err(|_| "Проверка обновления завершилась с ошибкой.".to_string())?
}

#[tauri::command]
pub(crate) async fn install_launcher_update(
    app: AppHandle,
    updater: State<'_, LauncherUpdater>,
    operations: State<'_, LauncherOperations>,
) -> Result<(), String> {
    if let Some(reason) = updater::unsupported_reason(&app) {
        return Err(reason);
    }
    let permit = updater
        .operation
        .acquire("Проверка или установка обновления")
        .map_err(|_| "Проверка или установка обновления уже выполняется.".to_string())?;
    let candidate = {
        let state = updater
            .state
            .lock()
            .map_err(|_| "Состояние обновления недоступно.".to_string())?;
        if state.stage == Stage::Ready {
            return Err("Обновление уже установлено. Перезапустите лаунчер.".into());
        }
        state
            .candidate
            .clone()
            .ok_or("Сначала проверьте наличие обновлений.")?
    };
    let destination = updater::installation_path(&app)?;
    let mutation_permits = operations.acquire_update()
        .map_err(|_| "Закройте Minecraft и дождитесь завершения установки или входа в аккаунт перед обновлением.".to_string())?;
    let updater = updater.inner().clone();
    updater.set_stage(Stage::Downloading)?;
    tauri::async_runtime::spawn(async move {
        let _permit = permit;
        let result = async {
            let key = updater::public_key(&app)?;
            let _ = app.emit(
                "launcher-update-progress",
                UpdateProgress {
                    stage: Stage::Downloading,
                    downloaded_bytes: 0,
                    total_bytes: None,
                },
            );
            let bytes =
                updater::download_verified(&candidate, &key, |downloaded_bytes, total_bytes| {
                    let _ = app.emit(
                        "launcher-update-progress",
                        UpdateProgress {
                            stage: Stage::Downloading,
                            downloaded_bytes,
                            total_bytes,
                        },
                    );
                })
                .await?;
            let size = bytes.len() as u64;
            updater.set_stage(Stage::Installing)?;
            let _ = app.emit(
                "launcher-update-progress",
                UpdateProgress {
                    stage: Stage::Installing,
                    downloaded_bytes: size,
                    total_bytes: Some(size),
                },
            );
            tauri::async_runtime::spawn_blocking(move || {
                updater::install_verified(&candidate, &bytes, &key, destination.as_deref())
            })
            .await
            .map_err(|_| "Установка обновления завершилась с ошибкой.".to_string())??;
            Ok::<_, String>(size)
        }
        .await;
        match result {
            Ok(size) => {
                let mut state = updater
                    .state
                    .lock()
                    .map_err(|_| "Состояние обновления недоступно.".to_string())?;
                state.stage = Stage::Ready;
                state.restart_permits = Some(mutation_permits);
                let _ = app.emit(
                    "launcher-update-progress",
                    UpdateProgress {
                        stage: Stage::Ready,
                        downloaded_bytes: size,
                        total_bytes: Some(size),
                    },
                );
                Ok(())
            }
            Err(error) => {
                updater.set_stage(Stage::Available)?;
                Err(error)
            }
        }
    })
    .await
    .map_err(|_| "Установка обновления завершилась с ошибкой.".to_string())?
}

#[tauri::command]
pub(crate) fn restart_launcher_after_update(
    app: AppHandle,
    updater: State<'_, LauncherUpdater>,
) -> Result<(), String> {
    let _permit = updater
        .operation
        .acquire("Установка обновления")
        .map_err(|_| "Установка обновления ещё выполняется.".to_string())?;
    {
        let state = updater
            .state
            .lock()
            .map_err(|_| "Состояние обновления недоступно.".to_string())?;
        if state.stage != Stage::Ready || state.restart_permits.is_none() {
            return Err("Сначала установите обновление лаунчера.".into());
        }
    }
    app.restart()
}
