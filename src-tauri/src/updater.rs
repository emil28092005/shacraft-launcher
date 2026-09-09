//! Native-only launcher updater. The webview controls timing, never trust inputs.
mod format;
mod protocol;
use crate::{
    operations::{LauncherOperations, Operation, Permit},
    update_guard::UpdateGuard,
};
use protocol::{Artifact, VerifiedRelease};
use reqwest::blocking::Client;
use serde::Serialize;
use std::{
    path::Path,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

#[derive(Clone, Serialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    Idle,
    Checking,
    Available,
    Downloading,
    Verifying,
    Installing,
    Ready,
    NoUpdate,
    Unconfigured,
    Manual,
    Error,
}

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateStatus {
    revision: u64,
    installed_version: String,
    package_format: &'static str,
    phase: Phase,
    available_version: Option<String>,
    release_notes: Option<String>,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    can_retry: bool,
    message: Option<String>,
    test_build: bool,
}

struct StateData {
    status: UpdateStatus,
    candidate: Option<VerifiedRelease>,
}

#[derive(Clone)]
pub(crate) struct UpdaterState {
    inner: Arc<Mutex<StateData>>,
    operation: Operation,
}
impl Default for UpdaterState {
    fn default() -> Self {
        let configured = configured_key().is_some();
        Self {
            inner: Arc::new(Mutex::new(StateData {
                status: UpdateStatus {
                    revision: 0,
                    installed_version: env!("CARGO_PKG_VERSION").into(),
                    package_format: format::installed_label(),
                    phase: if configured { Phase::Idle } else { Phase::Unconfigured },
                    available_version: None, release_notes: None, downloaded_bytes: 0, total_bytes: None,
                    can_retry: false,
                    message: (!configured).then(|| "Подписанные обновления ещё не настроены для этой сборки. Официальные выпуски доступны на GitHub.".into()),
                    test_build: protocol::test_build(),
                },
                candidate: None,
            })),
            operation: Operation::default(),
        }
    }
}

pub(crate) fn configured_key() -> Option<&'static str> {
    protocol::configured_key()
}

impl UpdaterState {
    pub fn status(&self) -> UpdateStatus {
        self.inner.lock().unwrap().status.clone()
    }
    pub fn acquire(&self) -> Result<Permit, String> {
        self.operation.acquire("Обновление лаунчера")
    }
    fn change(&self, app: &AppHandle, update: impl FnOnce(&mut StateData)) -> UpdateStatus {
        let status = {
            let mut data = self.inner.lock().unwrap();
            update(&mut data);
            data.status.revision += 1;
            data.status.clone()
        };
        let _ = app.emit("launcher-update-status", &status);
        status
    }
    fn phase(&self, app: &AppHandle, phase: Phase, message: Option<String>) -> UpdateStatus {
        self.change(app, |data| {
            data.status.can_retry = phase == Phase::Error;
            data.status.phase = phase;
            data.status.message = message;
        })
    }
    pub fn fail(&self, app: &AppHandle, error: String) -> UpdateStatus {
        self.change(app, |data| {
            data.status.can_retry = data.status.phase != Phase::Installing;
            data.status.phase = Phase::Error;
            data.status.message = Some(error);
            if !data.status.can_retry {
                data.candidate = None;
            }
        })
    }
    pub fn recovery(&self, app: &AppHandle, reason: String) -> UpdateStatus {
        self.change(app, |data| {
            data.candidate = None;
            data.status.phase = Phase::Error;
            data.status.can_retry = false;
            data.status.message = Some(reason);
        })
    }
    pub fn may_check(&self) -> bool {
        !matches!(
            self.status().phase,
            Phase::Downloading | Phase::Verifying | Phase::Installing | Phase::Ready
        )
    }
    pub fn ready(&self) -> bool {
        self.status().phase == Phase::Ready
    }
    pub fn critical(&self) -> bool {
        matches!(
            self.status().phase,
            Phase::Downloading | Phase::Verifying | Phase::Installing | Phase::Ready
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
enum PackageMode {
    Automatic { platform: &'static str, msi: bool },
    Manual,
}

fn package_mode(
    os: &str,
    arch: &str,
    bundle: Option<tauri::utils::config::BundleType>,
) -> PackageMode {
    use tauri::utils::config::BundleType;
    match (os, arch, bundle) {
        ("linux", "x86_64", Some(BundleType::AppImage)) => PackageMode::Automatic {
            platform: "linux-x86_64",
            msi: false,
        },
        ("windows", "x86_64", Some(BundleType::Nsis)) => PackageMode::Automatic {
            platform: "windows-x86_64",
            msi: false,
        },
        ("windows", "x86_64", Some(BundleType::Msi)) => PackageMode::Automatic {
            platform: "windows-x86_64",
            msi: true,
        },
        ("macos", "aarch64", Some(BundleType::App)) => PackageMode::Automatic {
            platform: "darwin-aarch64",
            msi: false,
        },
        ("macos", "x86_64", Some(BundleType::App)) => PackageMode::Automatic {
            platform: "darwin-x86_64",
            msi: false,
        },
        _ => PackageMode::Manual,
    }
}

fn current_mode() -> PackageMode {
    // Bare binaries, distro packages and dev runs must never be overwritten as an AppImage/.app.
    if cfg!(debug_assertions) {
        return PackageMode::Manual;
    }
    #[cfg(target_os = "macos")]
    {
        let app_bundle = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|p| p.ends_with("Contents/MacOS")))
            .unwrap_or(false);
        if !app_bundle {
            return PackageMode::Manual;
        }
    }
    package_mode(
        std::env::consts::OS,
        std::env::consts::ARCH,
        tauri::utils::platform::bundle_type(),
    )
}

fn client() -> Result<Client, String> {
    Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .user_agent("ShaCraft-Launcher-Updater/1")
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() <= 5 && protocol::redirect_allowed(attempt.url()) {
                attempt.follow()
            } else {
                attempt.error("Untrusted update redirect")
            }
        }))
        .build()
        .map_err(|_| "Не удалось подготовить соединение для обновлений".into())
}

fn response_bytes(
    client: &Client,
    url: &str,
    limit: u64,
    progress: impl FnMut(u64),
) -> Result<Option<Vec<u8>>, String> {
    let response = client
        .get(url)
        .send()
        .map_err(|_| "Не удалось связаться с GitHub. Проверьте сеть и повторите проверку.")?;
    if response.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(format!(
            "GitHub не отдал обновление (HTTP {}). Повторите позже.",
            response.status().as_u16()
        ));
    }
    if response.content_length().is_some_and(|size| size > limit) {
        return Err("Размер ответа превышает подписанный предел".into());
    }
    protocol::read_bounded(response, limit, progress).map(Some)
}

pub(crate) fn check(app: &AppHandle, state: &UpdaterState) -> Result<UpdateStatus, String> {
    if !state.may_check() {
        return Err("Сначала завершите текущее обновление лаунчера".into());
    }
    let Some(key) = configured_key() else {
        return Ok(state.phase(
            app,
            Phase::Unconfigured,
            Some("Подписанные обновления ещё не настроены для этой сборки.".into()),
        ));
    };
    state.change(app, |data| {
        data.candidate = None;
        data.status.phase = Phase::Checking;
        data.status.can_retry = false;
        data.status.message = None;
        data.status.downloaded_bytes = 0;
        data.status.total_bytes = None;
        data.status.available_version = None;
        data.status.release_notes = None;
    });
    let client = client()?;
    let Some(verified) = protocol::fetch_release(
        |url, limit| response_bytes(&client, url, limit, |_| {}),
        key,
        env!("CARGO_PKG_VERSION"),
    )?
    else {
        return Ok(state.phase(app, Phase::NoUpdate, None));
    };
    let manual = current_mode() == PackageMode::Manual;
    Ok(state.change(app, |data| {
        data.status.available_version = Some(verified.release.version.clone());
        data.status.release_notes = Some(verified.release.notes.clone());
        data.status.phase = if manual { Phase::Manual } else { Phase::Available };
        data.status.message = manual.then(|| "Эта сборка обновляется вручную. Для .deb используйте менеджер пакетов; AppImage и установленные Windows/macOS пакеты поддерживают обновление внутри лаунчера.".into());
        data.candidate = Some(verified);
    }))
}

fn select_artifact(release: &VerifiedRelease, mode: &PackageMode) -> Result<Artifact, String> {
    match mode {
        PackageMode::Automatic { platform, msi } => {
            let artifact = if *msi {
                release.release.manual_packages.get("windows-x86_64-msi")
            } else {
                release.release.platforms.get(*platform)
            };
            artifact
                .cloned()
                .ok_or_else(|| "Нет подписанного пакета для текущей платформы".into())
        }
        PackageMode::Manual => {
            Err("Эту сборку необходимо обновить вручную через официальный выпуск".into())
        }
    }
}

pub(crate) fn download_install(
    app: &AppHandle,
    state: &UpdaterState,
    directory: &Path,
    operations: &LauncherOperations,
) -> Result<UpdateStatus, String> {
    let key = configured_key().ok_or("Подписанные обновления не настроены")?;
    let release = state
        .inner
        .lock()
        .unwrap()
        .candidate
        .clone()
        .ok_or("Сначала проверьте доступность обновления")?;
    if !release.is_newer_than(env!("CARGO_PKG_VERSION"))? {
        return Err("Эта версия уже установлена".into());
    }
    let mode = current_mode();
    let artifact = select_artifact(&release, &mode)?;
    // Includes cross-process game lease and lifecycle exclusion; held through restart/handoff.
    let guard = UpdateGuard::acquire(directory, operations)?;
    state.change(app, |data| {
        data.status.phase = Phase::Downloading;
        data.status.can_retry = false;
        data.status.message = None;
        data.status.downloaded_bytes = 0;
        data.status.total_bytes = Some(artifact.size);
    });
    let client = client()?;
    let mut last = Instant::now();
    let bytes = response_bytes(&client, &artifact.url, artifact.size, |count| {
        if last.elapsed() >= Duration::from_millis(100) || count == artifact.size {
            state.change(app, |data| {
                data.status.downloaded_bytes = count;
            });
            last = Instant::now();
        }
    })?
    .ok_or("Сервер не вернул пакет обновления")?;
    state.phase(app, Phase::Verifying, None);
    protocol::verify_artifact(&bytes, &artifact, key)?;
    format::verify(&mode, &bytes)?;
    let platform = match mode {
        PackageMode::Automatic { platform, .. } => platform,
        PackageMode::Manual => unreachable!(),
    };
    // The plugin's constructor is private. Its check creates the native installer
    // context from a fixed version URL; accept only the signed metadata already read.
    let builder = app
        .updater_builder()
        .pubkey(key)
        .target(platform)
        .endpoints(vec![release
            .pinned_endpoint()
            .parse()
            .map_err(|_| "Некорректный адрес выпуска")?])
        .map_err(|_| "Не удалось настроить установщик обновления")?
        .configure_client(|builder| {
            builder
                .https_only(true)
                .connect_timeout(Duration::from_secs(15))
                .timeout(Duration::from_secs(30))
                .redirect(reqwest_updater::redirect::Policy::custom(|attempt| {
                    if attempt.previous().len() <= 5 && protocol::redirect_allowed(attempt.url()) {
                        attempt.follow()
                    } else {
                        attempt.error("Untrusted update redirect")
                    }
                }))
        });
    let mut update = tauri::async_runtime::block_on(
        builder
            .build()
            .map_err(|_| "Не удалось подготовить установщик")?
            .check(),
    )
    .map_err(|_| "Не удалось сверить подписанный выпуск с установщиком. Повторите попытку.")?
    .ok_or("Подписанный выпуск больше не доступен установщику")?;
    if update.raw_json != release.json
        || update.version != release.release.version
        || update.target != platform
    {
        return Err(
            "Метаданные выпуска изменились. Установка остановлена; проверьте обновления заново."
                .into(),
        );
    }
    // MSI installations stay MSI; the signed descriptor is never supplied by JS.
    update.download_url = artifact
        .url
        .parse()
        .map_err(|_| "Некорректный адрес пакета")?;
    update.signature = artifact.signature.clone();
    // Update::install does NOT verify bytes itself. Keep this immediately before it.
    protocol::install_verified(&bytes, &artifact, key, |verified_bytes| {
        guard.begin_install(env!("CARGO_PKG_VERSION"), &release.release.version)?;
        state.phase(
            app,
            Phase::Installing,
            Some("Лаунчер перезапустится после установки. Не выключайте компьютер.".into()),
        );
        update.install(verified_bytes).map_err(|_| "Установка прервалась. Запись о незавершённом обновлении сохранена; следуйте инструкции восстановления.".to_string())
    })?;
    // Windows exits inside plugin install after handing off to NSIS/MSI. There is
    // no installer PID API; the persistent lifecycle marker guards the new process.
    state.phase(
        app,
        Phase::Ready,
        Some("Обновление установлено. Перезапускаем лаунчер.".into()),
    );
    app.restart()
}

pub(crate) fn open_release_page() -> Result<(), String> {
    // Fixed executable/argument structure and URL; no shell or webview-supplied input.
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(protocol::RELEASES);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(protocol::RELEASES);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("rundll32.exe");
        command.args(["url.dll,FileProtocolHandler", protocol::RELEASES]);
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|_| "Не удалось открыть страницу официальных выпусков".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::utils::config::BundleType;
    #[test]
    fn package_formats_never_cross_installers_or_architectures() {
        assert_eq!(
            package_mode("linux", "x86_64", Some(BundleType::Deb)),
            PackageMode::Manual
        );
        assert_eq!(package_mode("linux", "x86_64", None), PackageMode::Manual);
        assert_eq!(
            package_mode("windows", "aarch64", Some(BundleType::Nsis)),
            PackageMode::Manual
        );
        assert_eq!(
            package_mode("windows", "x86_64", Some(BundleType::Msi)),
            PackageMode::Automatic {
                platform: "windows-x86_64",
                msi: true
            }
        );
        assert_eq!(
            package_mode("macos", "aarch64", Some(BundleType::App)),
            PackageMode::Automatic {
                platform: "darwin-aarch64",
                msi: false
            }
        );
        assert_eq!(
            package_mode("macos", "x86_64", Some(BundleType::App)),
            PackageMode::Automatic {
                platform: "darwin-x86_64",
                msi: false
            }
        );
    }
    #[test]
    fn updater_single_flight_releases_after_failure() {
        let state = UpdaterState::default();
        let permit = state.acquire().unwrap();
        assert!(state.acquire().is_err());
        drop(permit);
        assert!(state.acquire().is_ok());
    }
}
