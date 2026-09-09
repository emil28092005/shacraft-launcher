//! No updater command accepts a URL, key, target, version, executable or arguments.
use super::data_dir;
use crate::{
    operations::LauncherOperations,
    update_guard,
    updater::{self, UpdateStatus, UpdaterState},
};
use tauri::{AppHandle, Manager, State};

fn pending(app: &AppHandle, state: &UpdaterState) -> Result<Option<UpdateStatus>, String> {
    let operations = app.state::<LauncherOperations>();
    if let Err(reason) = operations.ensure_writable() {
        return Ok(Some(state.recovery(app, reason)));
    }
    let directory = data_dir(app)?;
    match update_guard::pending_reason(&directory, env!("CARGO_PKG_VERSION")) {
        Ok(Some(reason)) | Err(reason) => {
            // Recovery discovered after startup is just as permanent for this
            // process. External marker deletion cannot resume native writes.
            operations.latch_recovery(reason.clone());
            Ok(Some(state.recovery(app, reason)))
        }
        Ok(None) => Ok(None),
    }
}

fn contain_worker_panic<T>(
    work: impl FnOnce() -> Result<T, String>,
    panic_message: &str,
) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
        .unwrap_or_else(|_| Err(panic_message.to_string()))
}

fn report_failure(app: &AppHandle, state: &UpdaterState, error: String) -> UpdateStatus {
    match pending(app, state) {
        Ok(Some(recovery)) => recovery,
        Ok(None) => state.fail(app, error),
        Err(reason) => {
            app.state::<LauncherOperations>()
                .latch_recovery(reason.clone());
            state.recovery(app, reason)
        }
    }
}

#[tauri::command]
pub(crate) fn updater_status(
    app: AppHandle,
    state: State<'_, UpdaterState>,
) -> Result<UpdateStatus, String> {
    if state.critical() {
        return Ok(state.status());
    }
    Ok(pending(&app, &state)?.unwrap_or_else(|| state.status()))
}

#[tauri::command]
pub(crate) async fn updater_check(
    app: AppHandle,
    state: State<'_, UpdaterState>,
) -> Result<UpdateStatus, String> {
    let permit = state.acquire()?;
    if let Some(status) = pending(&app, &state)? {
        return Ok(status);
    }
    let state = state.inner().clone();
    let worker_app = app.clone();
    let worker_state = state.clone();
    let worker = tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        // Publish failure from the worker even if its IPC caller disappeared.
        contain_worker_panic(
            || updater::check(&worker_app, &worker_state),
            "Проверка обновления прервалась. Повторите проверку.",
        )
        .unwrap_or_else(|error| report_failure(&worker_app, &worker_state, error))
    });
    Ok(worker.await.unwrap_or_else(|_| {
        report_failure(
            &app,
            &state,
            "Проверка обновления прервалась. Повторите проверку.".into(),
        )
    }))
}

#[tauri::command]
pub(crate) async fn updater_download_install(
    app: AppHandle,
    state: State<'_, UpdaterState>,
) -> Result<UpdateStatus, String> {
    let permit = state.acquire()?;
    if let Some(status) = pending(&app, &state)? {
        return Ok(status);
    }
    let directory = data_dir(&app)?;
    let state = state.inner().clone();
    let worker_app = app.clone();
    let worker_state = state.clone();
    let worker = tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        contain_worker_panic(
            || {
                let operations = worker_app.state::<LauncherOperations>();
                updater::download_install(&worker_app, &worker_state, &directory, &operations)
            },
            "Установка обновления прервалась; проверьте состояние перед повторной попыткой.",
        )
        .unwrap_or_else(|error| report_failure(&worker_app, &worker_state, error))
    });
    Ok(worker.await.unwrap_or_else(|_| {
        report_failure(
            &app,
            &state,
            "Установка обновления прервалась; проверьте состояние перед повторной попыткой.".into(),
        )
    }))
}

#[tauri::command]
pub(crate) fn updater_restart(
    app: AppHandle,
    state: State<'_, UpdaterState>,
) -> Result<(), String> {
    let _permit = state.acquire()?;
    if !state.ready() {
        return Err("Нет завершённого обновления для перезапуска".into());
    }
    app.restart()
}

#[tauri::command]
pub(crate) fn updater_open_release_page() -> Result<(), String> {
    updater::open_release_page()
}

#[cfg(test)]
mod tests {
    use super::contain_worker_panic;
    use crate::operations::Operation;

    #[test]
    fn panicking_worker_releases_owned_permit_and_returns_a_retryable_boundary_error() {
        let operation = Operation::default();
        let permit = operation.acquire("test updater").unwrap();
        let result: Result<(), String> = contain_worker_panic(
            move || {
                let _permit = permit;
                panic!("synthetic worker failure");
            },
            "worker interrupted",
        );
        assert_eq!(result, Err("worker interrupted".into()));
        assert!(operation.acquire("retry").is_ok());
        assert_eq!(contain_worker_panic(|| Ok(42), "unused"), Ok(42));
    }
}
