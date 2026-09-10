//! Launcher releases form a separate trust boundary from modpack manifests.
//! Only this module chooses update URLs. The webview receives display data and
//! progress, never an updater resource, destination, signature or public key.
use crate::operations::{Operation, UpdatePermits};
use base64::{engine::general_purpose::STANDARD, Engine};
use minisign_verify::{PublicKey, Signature};
use serde::Serialize;
use serde_json::Value;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_updater::{Update, UpdaterBuilder, UpdaterExt};
use url::Url;

pub(crate) const UPDATE_ENDPOINT: &str = "https://shacraft.ru/launcher/updates/stable.json";
pub(crate) const MAX_METADATA_BYTES: usize = 192 * 1024;
const MAX_PAYLOAD_BYTES: usize = 64 * 1024;
pub(crate) const MAX_ARTIFACT_BYTES: usize = 256 * 1024 * 1024;
const BAD_METADATA: &str =
    "Не удалось подтвердить подлинность сведений об обновлении. Повторите проверку позже.";
const BAD_SIGNATURE: &str = "Подпись обновления не прошла проверку. Установка отменена.";

#[derive(Clone, Copy, Serialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub(crate) enum InstallationKind {
    Appimage,
    Deb,
    Other,
}

#[derive(Clone, Copy, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Stage {
    #[default]
    Idle,
    Checking,
    Available,
    Downloading,
    Installing,
    Ready,
}

#[derive(Default)]
pub(crate) struct UpdateState {
    pub stage: Stage,
    pub candidate: Option<Update>,
    // Hold all mutation gates until the user restarts into the installed app.
    pub restart_permits: Option<UpdatePermits>,
}

#[derive(Clone, Default)]
pub(crate) struct LauncherUpdater {
    pub operation: Operation,
    pub state: Arc<Mutex<UpdateState>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateStatus {
    pub current_version: String,
    pub supported: bool,
    pub installation_kind: InstallationKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub stage: Stage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateProgress {
    pub stage: Stage,
    pub downloaded_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
}

impl LauncherUpdater {
    pub fn status<R: Runtime>(&self, app: &AppHandle<R>) -> Result<UpdateStatus, String> {
        let reason = unsupported_reason(app);
        let state = self
            .state
            .lock()
            .map_err(|_| "Состояние обновления недоступно.".to_string())?;
        Ok(UpdateStatus {
            current_version: app.package_info().version.to_string(),
            supported: reason.is_none(),
            installation_kind: installation_kind(app),
            reason,
            stage: state.stage,
            version: state
                .candidate
                .as_ref()
                .map(|update| update.version.clone()),
            notes: state
                .candidate
                .as_ref()
                .and_then(|update| update.body.clone()),
        })
    }

    pub fn set_stage(&self, stage: Stage) -> Result<(), String> {
        self.state
            .lock()
            .map_err(|_| "Состояние обновления недоступно.".to_string())?
            .stage = stage;
        Ok(())
    }
}

pub(crate) fn installation_kind<R: Runtime>(app: &AppHandle<R>) -> InstallationKind {
    #[cfg(target_os = "linux")]
    {
        let env = app.env();
        if let (Some(image), Some(directory), Ok(executable)) = (
            env.appimage.as_ref(),
            env.appdir.as_ref(),
            std::env::current_exe(),
        ) {
            if linux_appimage_supported(image.as_ref(), directory.as_ref(), &executable) {
                return InstallationKind::Appimage;
            }
        }
        if crate::deb_updater::installed_binary_supported() {
            return InstallationKind::Deb;
        }
    }
    let _ = app;
    InstallationKind::Other
}

pub(crate) fn unsupported_reason<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    #[cfg(target_os = "linux")]
    match installation_kind(app) {
        InstallationKind::Appimage => return None,
        InstallationKind::Deb => return crate::deb_updater::unsupported_reason(),
        InstallationKind::Other => return Some("Для автообновления установите deb-пакет или запустите AppImage с shacraft.ru/help#launcher.".into()),
    }
    #[cfg(not(target_os = "linux"))]
    let _ = app;
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    return Some("Для этой платформы доступна только ручная установка обновлений.".into());
    #[cfg(not(target_os = "linux"))]
    None
}

#[cfg(target_os = "linux")]
fn linux_appimage_supported(
    image: &std::path::Path,
    directory: &std::path::Path,
    executable: &std::path::Path,
) -> bool {
    use std::io::Read;
    if !image.is_absolute()
        || !directory.is_absolute()
        || !executable.starts_with(directory)
        || !image.is_file()
    {
        return false;
    }
    let mut header = [0_u8; 11];
    std::fs::File::open(image)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok()
        && &header[..4] == b"\x7fELF"
        && &header[8..11] == b"AI\x02"
}

pub(crate) fn public_key<R: Runtime>(app: &AppHandle<R>) -> Result<String, String> {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|value| value.get("pubkey"))
        .and_then(Value::as_str)
        .filter(|key| !key.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "В этой сборке отсутствует ключ проверки обновлений.".into())
}

/// Resolve the startup AppImage path once. Release metadata and IPC never
/// select an installation destination; symlink launch shortcuts remain usable.
pub(crate) fn installation_path<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<Option<std::path::PathBuf>, String> {
    #[cfg(target_os = "linux")]
    {
        if installation_kind(app) == InstallationKind::Deb {
            return Ok(None);
        }
        let image = app
            .env()
            .appimage
            .ok_or("Запустите AppImage, чтобы обновить лаунчер.")?;
        return std::fs::canonicalize(image)
            .map(Some)
            .map_err(|_| "Файл AppImage перемещён или недоступен. Запустите его снова.".into());
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = app;
        Ok(None)
    }
}

pub(crate) fn trusted_builder<R: Runtime>(app: &AppHandle<R>) -> Result<UpdaterBuilder, String> {
    app.updater_builder()
        .endpoints(vec![Url::parse(UPDATE_ENDPOINT).expect("fixed update URL")])
        .map_err(|_| BAD_METADATA.to_string())
        .map(|builder| {
            builder
                .timeout(Duration::from_secs(20))
                .configure_client(|client| {
                    client
                        .https_only(true)
                        .redirect(reqwest_updater::redirect::Policy::none())
                        .connect_timeout(Duration::from_secs(10))
                        .timeout(Duration::from_secs(20))
                        .danger_accept_invalid_certs(false)
                        .danger_accept_invalid_hostnames(false)
                })
        })
}

fn http_client(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(timeout)
        .user_agent(concat!("ShaCraft-Launcher/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| "Не удалось подключиться к серверу обновлений.".into())
}

fn checked_length(current: usize, next: usize, maximum: usize) -> Result<usize, String> {
    current
        .checked_add(next)
        .filter(|size| *size <= maximum)
        .ok_or_else(|| "Размер ответа сервера обновлений превышает допустимый.".into())
}

async fn bounded_response(
    mut response: reqwest::Response,
    maximum: usize,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<Vec<u8>, String> {
    if !response.status().is_success() || response.status() == reqwest::StatusCode::NO_CONTENT {
        return Err("Сервер обновлений временно недоступен. Повторите попытку позже.".into());
    }
    let total = response.content_length();
    if total.is_some_and(|length| length > maximum as u64) {
        return Err("Размер ответа сервера обновлений превышает допустимый.".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| {
        "Загрузка обновления прервалась. Проверьте соединение и повторите попытку.".to_string()
    })? {
        checked_length(bytes.len(), chunk.len(), maximum)?;
        bytes.extend_from_slice(&chunk);
        progress(bytes.len() as u64, total);
    }
    if total.is_some_and(|length| length != bytes.len() as u64) {
        return Err("Обновление загружено не полностью. Повторите попытку.".into());
    }
    Ok(bytes)
}

/// Same Minisign format and verification semantics as Tauri's updater. The
/// signed metadata and artifact each need a valid signature under the embedded
/// release key. A signed old artifact cannot be labelled as a new version.
pub(crate) fn verify_signature(
    bytes: &[u8],
    encoded_signature: &str,
    encoded_key: &str,
) -> Result<(), String> {
    if encoded_signature.len() > 4096 || encoded_key.len() > 4096 {
        return Err(BAD_SIGNATURE.into());
    }
    let key_bytes = STANDARD.decode(encoded_key).map_err(|_| BAD_SIGNATURE)?;
    let signature_bytes = STANDARD
        .decode(encoded_signature)
        .map_err(|_| BAD_SIGNATURE)?;
    let key = PublicKey::decode(std::str::from_utf8(&key_bytes).map_err(|_| BAD_SIGNATURE)?)
        .map_err(|_| BAD_SIGNATURE)?;
    let signature =
        Signature::decode(std::str::from_utf8(&signature_bytes).map_err(|_| BAD_SIGNATURE)?)
            .map_err(|_| BAD_SIGNATURE)?;
    key.verify(bytes, &signature, true)
        .map_err(|_| BAD_SIGNATURE.into())
}

pub(crate) fn verified_metadata(raw: &Value, key: &str) -> Result<Value, String> {
    let object = raw.as_object().ok_or(BAD_METADATA)?;
    if object.len() != 6 {
        return Err(BAD_METADATA.into());
    }
    let encoded = raw
        .get("signedPayload")
        .and_then(Value::as_str)
        .ok_or(BAD_METADATA)?;
    if encoded.len() > MAX_PAYLOAD_BYTES * 4 / 3 + 4 {
        return Err(BAD_METADATA.into());
    }
    let payload = STANDARD.decode(encoded).map_err(|_| BAD_METADATA)?;
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(BAD_METADATA.into());
    }
    let signature = raw
        .get("metadataSignature")
        .and_then(Value::as_str)
        .ok_or(BAD_METADATA)?;
    verify_signature(&payload, signature, key).map_err(|_| BAD_METADATA)?;
    let parsed: Value = serde_json::from_slice(&payload).map_err(|_| BAD_METADATA)?;
    let signed = parsed.as_object().ok_or(BAD_METADATA)?;
    if signed.len() != 4 {
        return Err(BAD_METADATA.into());
    }
    for field in ["version", "notes", "pub_date", "platforms"] {
        if !signed.contains_key(field) || signed.get(field) != object.get(field) {
            return Err(BAD_METADATA.into());
        }
    }
    Ok(parsed)
}

pub(crate) fn newer_version(metadata: &Value, current: &str) -> Result<bool, String> {
    let announced = metadata
        .get("version")
        .and_then(Value::as_str)
        .ok_or(BAD_METADATA)?;
    let version = semver::Version::parse(announced).map_err(|_| BAD_METADATA)?;
    // Stable channel rejects prerelease/build aliases and noncanonical spellings.
    if !version.pre.is_empty() || !version.build.is_empty() || version.to_string() != announced {
        return Err(BAD_METADATA.into());
    }
    let current = semver::Version::parse(current).map_err(|_| BAD_METADATA)?;
    Ok(version > current)
}

fn require_platform(metadata: &Value, kind: InstallationKind) -> Result<String, String> {
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else {
        std::env::consts::OS
    };
    let target = format!("{os}-{}", std::env::consts::ARCH);
    let platforms = metadata
        .get("platforms")
        .and_then(Value::as_object)
        .ok_or(BAD_METADATA)?;
    #[cfg(target_os = "linux")]
    let targets = match kind {
        InstallationKind::Deb => vec![format!("{target}-deb")],
        InstallationKind::Appimage => vec![format!("{target}-appimage"), target],
        InstallationKind::Other => {
            return Err("Формат установленного лаунчера не поддерживает обновление.".into())
        }
    };
    #[cfg(not(target_os = "linux"))]
    let targets = {
        let _ = kind;
        ["nsis", "msi", "app"]
            .iter()
            .map(|bundle| format!("{target}-{bundle}"))
            .chain(std::iter::once(target))
            .collect::<Vec<_>>()
    };
    targets
        .into_iter()
        .find(|target| platforms.contains_key(target))
        .ok_or_else(|| "Обновление для вашего формата установки пока не опубликовано.".into())
}

pub(crate) fn validate_download_url(
    url: &Url,
    version: &str,
    kind: InstallationKind,
) -> Result<(), String> {
    let prefix = format!("/downloads/shacraft-launcher/{version}/");
    let filename = url.path().strip_prefix(&prefix).ok_or(BAD_METADATA)?;
    if url.scheme() != "https"
        || url.host_str() != Some("shacraft.ru")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || filename.is_empty()
        || !filename.as_bytes()[0].is_ascii_alphanumeric()
        || !filename
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return Err(BAD_METADATA.into());
    }
    let correct_extension = if cfg!(target_os = "linux") {
        match kind {
            InstallationKind::Appimage => filename.ends_with(".AppImage"),
            InstallationKind::Deb => filename.ends_with(".deb"),
            InstallationKind::Other => false,
        }
    } else if cfg!(target_os = "macos") {
        filename.ends_with(".app.tar.gz")
    } else if cfg!(target_os = "windows") {
        filename.ends_with(".exe") || filename.ends_with(".msi")
    } else {
        false
    };
    if !correct_extension {
        return Err(BAD_METADATA.into());
    }
    Ok(())
}

fn candidate_kind(update: &Update) -> InstallationKind {
    if cfg!(target_os = "linux") {
        if update.target == format!("linux-{}-deb", std::env::consts::ARCH) {
            InstallationKind::Deb
        } else {
            InstallationKind::Appimage
        }
    } else {
        InstallationKind::Other
    }
}

/// Fetch and authenticate a bounded static manifest before asking the vendored
/// upstream plugin's small offline constructor to create an Update. Its normal
/// HTTP check is intentionally unused because it buffers unbounded JSON.
pub(crate) async fn check_candidate(
    builder: UpdaterBuilder,
    key: &str,
    current: &str,
    kind: InstallationKind,
) -> Result<Option<Update>, String> {
    let response = http_client(Duration::from_secs(20))?
        .get(UPDATE_ENDPOINT)
        .send()
        .await
        .map_err(|_| {
            "Не удалось проверить обновления. Проверьте подключение к интернету.".to_string()
        })?;
    let bytes = bounded_response(response, MAX_METADATA_BYTES, |_, _| {}).await?;
    let raw: Value = serde_json::from_slice(&bytes).map_err(|_| BAD_METADATA)?;
    let metadata = verified_metadata(&raw, key)?;
    let target = require_platform(&metadata, kind)?;
    if !newer_version(&metadata, current)? {
        return Ok(None);
    }
    #[cfg(target_os = "linux")]
    let builder = builder.target(target);
    #[cfg(not(target_os = "linux"))]
    let _ = target;
    let update = builder
        .build()
        .map_err(updater_error)?
        .check_metadata(raw.clone())
        .map_err(updater_error)?
        .ok_or(BAD_METADATA)?;
    if update.raw_json != raw
        || update.current_version != current
        || Some(update.version.as_str()) != metadata.get("version").and_then(Value::as_str)
    {
        return Err(BAD_METADATA.into());
    }
    // Retain the exact signed envelope with the native-only candidate.
    verified_metadata(&update.raw_json, key)?;
    validate_download_url(
        &update.download_url,
        &update.version,
        candidate_kind(&update),
    )?;
    Ok(Some(update))
}

pub(crate) async fn download_verified(
    update: &Update,
    key: &str,
    progress: impl FnMut(u64, Option<u64>),
) -> Result<Vec<u8>, String> {
    validate_download_url(
        &update.download_url,
        &update.version,
        candidate_kind(update),
    )?;
    verified_metadata(&update.raw_json, key)?;
    let response = http_client(Duration::from_secs(600))?
        .get(update.download_url.clone())
        .send()
        .await
        .map_err(|_| {
            "Не удалось загрузить обновление. Проверьте подключение и повторите попытку."
                .to_string()
        })?;
    let bytes = bounded_response(response, MAX_ARTIFACT_BYTES, progress).await?;
    verify_signature(&bytes, &update.signature, key)?;
    Ok(bytes)
}

/// Keep verification adjacent to the only call that can replace the app. This
/// also protects against an accidental mutation of bytes after downloading.
pub(crate) fn install_verified(
    update: &Update,
    bytes: &[u8],
    key: &str,
    destination: Option<&std::path::Path>,
) -> Result<(), String> {
    validate_download_url(
        &update.download_url,
        &update.version,
        candidate_kind(update),
    )?;
    verify_signature(bytes, &update.signature, key)?;
    #[cfg(target_os = "linux")]
    {
        if candidate_kind(update) == InstallationKind::Deb {
            return crate::deb_updater::install(update, bytes);
        }
        let destination = destination.ok_or("Файл AppImage недоступен.")?;
        install_appimage_atomic(destination, bytes).map_err(|_| {
            "Не удалось заменить AppImage. Проверьте свободное место и права на папку лаунчера."
                .into()
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = destination;
        update.install(bytes).map_err(updater_error)
    }
}

/// Tauri 2.11 moves the old AppImage away before writing the new one. Use our
/// atomic-file primitive on Linux so interruption during writing leaves the
/// old executable intact. Other OS installers stay with the official plugin.
#[cfg(target_os = "linux")]
fn install_appimage_atomic(destination: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::{
        fs,
        io::{self, Write},
        os::unix::fs::PermissionsExt,
    };
    if bytes.len() < 11 || &bytes[..4] != b"\x7fELF" || &bytes[8..11] != b"AI\x02" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "release is not a type-2 AppImage",
        ));
    }
    let metadata = fs::symlink_metadata(destination)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "AppImage must be a regular file",
        ));
    }
    let parent =
        fs::File::open(destination.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "AppImage has no parent")
        })?)?;
    let mut output = crate::storage::AtomicFile::new(destination)?;
    output.writer().write_all(bytes)?;
    output.writer().set_permissions(fs::Permissions::from_mode(
        metadata.permissions().mode() & 0o777,
    ))?;
    output.commit()?;
    parent.sync_all()
}

pub(crate) fn updater_error(error: tauri_plugin_updater::Error) -> String {
    match error {
        tauri_plugin_updater::Error::TargetNotFound(_) | tauri_plugin_updater::Error::TargetsNotFound(_) =>
            "Обновление для вашей платформы пока не опубликовано.".into(),
        tauri_plugin_updater::Error::Io(_) | tauri_plugin_updater::Error::TempDirNotOnSameMountPoint =>
            "Не удалось заменить файл лаунчера. Проверьте свободное место и права на папку приложения.".into(),
        tauri_plugin_updater::Error::Minisign(_) | tauri_plugin_updater::Error::Base64(_) |
        tauri_plugin_updater::Error::SignatureUtf8(_) => BAD_SIGNATURE.into(),
        _ => "Не удалось установить обновление. Повторите попытку или скачайте лаунчер с shacraft.ru.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact_url() -> &'static str {
        if cfg!(target_os = "linux") {
            "https://shacraft.ru/downloads/shacraft-launcher/0.2.0/ShaCraft_0.2.0.AppImage"
        } else if cfg!(target_os = "macos") {
            "https://shacraft.ru/downloads/shacraft-launcher/0.2.0/ShaCraft_0.2.0.app.tar.gz"
        } else {
            "https://shacraft.ru/downloads/shacraft-launcher/0.2.0/ShaCraft_0.2.0.exe"
        }
    }

    #[test]
    fn download_policy_pins_origin_version_plain_path_and_package_type() {
        assert!(validate_download_url(
            &Url::parse(artifact_url()).unwrap(),
            "0.2.0",
            InstallationKind::Appimage
        )
        .is_ok());
        for value in [
            artifact_url().replace("https:", "http:"),
            artifact_url().replace("shacraft.ru/", "evil.example/"),
            artifact_url().replace("shacraft.ru/", "shacraft.ru:8443/"),
            artifact_url().replace("https://", "https://user@"),
            format!("{}?url=x", artifact_url()),
            format!("{}#x", artifact_url()),
            artifact_url().replace("/0.2.0/", "/0.1.0/"),
            artifact_url().replace("ShaCraft_", "%53haCraft_"),
            artifact_url().replace("ShaCraft_", "nested/ShaCraft_"),
            format!("{}.sh", artifact_url()),
        ] {
            assert!(
                validate_download_url(
                    &Url::parse(&value).unwrap(),
                    "0.2.0",
                    InstallationKind::Appimage
                )
                .is_err(),
                "{value}"
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_selects_package_family_without_deb_fallback() {
        let base = format!("linux-{}", std::env::consts::ARCH);
        let legacy = serde_json::json!({"platforms": {base.clone(): {}}});
        assert_eq!(
            require_platform(&legacy, InstallationKind::Appimage).unwrap(),
            base
        );
        assert!(require_platform(&legacy, InstallationKind::Deb).is_err());
        let exact_image = format!("{base}-appimage");
        let exact_deb = format!("{base}-deb");
        let all = serde_json::json!({"platforms": {base.clone(): {}, exact_image.clone(): {}, exact_deb.clone(): {}}});
        assert_eq!(
            require_platform(&all, InstallationKind::Appimage).unwrap(),
            exact_image
        );
        assert_eq!(
            require_platform(&all, InstallationKind::Deb).unwrap(),
            exact_deb
        );
        let deb = Url::parse(
            "https://shacraft.ru/downloads/shacraft-launcher/0.2.0/ShaCraft_0.2.0_amd64.deb",
        )
        .unwrap();
        assert!(validate_download_url(&deb, "0.2.0", InstallationKind::Deb).is_ok());
        assert!(validate_download_url(&deb, "0.2.0", InstallationKind::Appimage).is_err());
        assert!(validate_download_url(
            &Url::parse(artifact_url()).unwrap(),
            "0.2.0",
            InstallationKind::Deb
        )
        .is_err());
    }

    #[test]
    fn stable_channel_never_downgrades_or_installs_equal_aliases() {
        assert!(newer_version(&serde_json::json!({"version":"0.2.0"}), "0.1.3").unwrap());
        for version in ["0.1.2", "0.1.3"] {
            assert!(!newer_version(&serde_json::json!({"version":version}), "0.1.3").unwrap());
        }
        for version in ["v0.2.0", "0.2.0-test", "0.2.0+extra", "00.2.0", "../0.2.0"] {
            assert!(newer_version(&serde_json::json!({"version":version}), "0.1.3").is_err());
        }
    }

    #[test]
    fn streaming_size_limit_handles_missing_length_and_overflow() {
        assert_eq!(checked_length(3, 5, 8).unwrap(), 8);
        assert!(checked_length(3, 6, 8).is_err());
        assert!(checked_length(usize::MAX, 1, usize::MAX).is_err());
    }

    #[test]
    fn missing_or_oversized_metadata_proof_is_rejected() {
        assert!(verified_metadata(&serde_json::json!({"version":"0.2.0"}), "").is_err());
        let raw = serde_json::json!({"version":"0.2.0","notes":"","pub_date":"","platforms":{},
            "signedPayload":"A".repeat(MAX_PAYLOAD_BYTES * 2),"metadataSignature":""});
        assert!(verified_metadata(&raw, "").is_err());
    }

    fn fixture() -> Value {
        // Public test key/signatures only. The ephemeral private key was
        // discarded by the publisher tests and never enters this repository.
        serde_json::from_str(include_str!("../tests/fixtures/updater-signed.json")).unwrap()
    }

    #[test]
    fn genuine_metadata_and_artifact_signatures_pass_and_tampering_fails() {
        let fixture = fixture();
        let key = fixture["publicKey"].as_str().unwrap();
        let raw = &fixture["metadata"];
        assert!(verified_metadata(raw, key).is_ok());
        let signature = raw["platforms"]["linux-x86_64"]["signature"]
            .as_str()
            .unwrap();
        let bytes = fixture["artifactText"].as_str().unwrap().as_bytes();
        assert!(verify_signature(bytes, signature, key).is_ok());
        let mut corrupt = bytes.to_vec();
        corrupt[0] ^= 1;
        assert!(verify_signature(&corrupt, signature, key).is_err());
        assert!(
            verify_signature(bytes, &STANDARD.encode("invalid minisign signature"), key).is_err()
        );
        assert!(
            verify_signature(bytes, signature, &STANDARD.encode("invalid public key")).is_err()
        );
    }

    #[test]
    fn relabelling_a_signed_old_artifact_or_changing_signed_fields_fails() {
        let fixture = fixture();
        let key = fixture["publicKey"].as_str().unwrap();
        for field in ["version", "notes", "pub_date", "platforms"] {
            let mut raw = fixture["metadata"].clone();
            raw[field] = Value::String("tampered".into());
            assert!(verified_metadata(&raw, key).is_err(), "{field}");
        }
        let mut raw = fixture["metadata"].clone();
        let mut payload: Value = serde_json::from_slice(
            &STANDARD
                .decode(raw["signedPayload"].as_str().unwrap())
                .unwrap(),
        )
        .unwrap();
        raw["version"] = Value::String("99.0.0".into());
        payload["version"] = Value::String("99.0.0".into());
        raw["signedPayload"] =
            Value::String(STANDARD.encode(serde_json::to_vec(&payload).unwrap()));
        assert!(verified_metadata(&raw, key).is_err());
    }

    #[test]
    fn unavailable_platform_is_not_reported_as_latest() {
        assert!(require_platform(
            &serde_json::json!({"platforms":{}}),
            InstallationKind::Appimage
        )
        .is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_support_requires_actual_appimage_and_matching_appdir() {
        let directory = tempfile::tempdir().unwrap();
        let image = directory.path().join("launcher.AppImage");
        let appdir = directory.path().join(".mount_test");
        let binary = appdir.join("usr/bin/shacraft-launcher");
        std::fs::write(&image, b"\x7fELF\x02\x01\x01\0AI\x02rest").unwrap();
        assert!(linux_appimage_supported(&image, &appdir, &binary));
        assert!(!linux_appimage_supported(
            &image,
            &appdir,
            &directory.path().join("raw-binary")
        ));
        std::fs::write(&image, b"not a valid AppImage").unwrap();
        assert!(!linux_appimage_supported(&image, &appdir, &binary));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn appimage_replacement_preserves_old_on_error_and_retains_executable_mode() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let image = directory.path().join("launcher.AppImage");
        let old = b"\x7fELF\x02\x01\x01\0AI\x02old";
        let new = b"\x7fELF\x02\x01\x01\0AI\x02new";
        std::fs::write(&image, old).unwrap();
        std::fs::set_permissions(&image, std::fs::Permissions::from_mode(0o751)).unwrap();
        assert!(install_appimage_atomic(&image, b"wrong package").is_err());
        assert_eq!(std::fs::read(&image).unwrap(), old);
        assert!(install_appimage_atomic(&directory.path().join("missing"), new).is_err());
        assert_eq!(std::fs::read(&image).unwrap(), old);
        install_appimage_atomic(&image, new).unwrap();
        assert_eq!(std::fs::read(&image).unwrap(), new);
        assert_eq!(
            std::fs::metadata(&image).unwrap().permissions().mode() & 0o777,
            0o751
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    /// Read-only production HTTPS requests; replacement occurs ONLY in a new
    /// temporary copy of the path supplied by the operator. No QA switches or
    /// alternative endpoints are compiled into a distributed launcher.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires published signed update and SHACRAFT_UPDATER_TEST_IMAGE pointing to an old AppImage"]
    fn live_signed_update_replaces_only_temporary_copy() {
        use sha2::{Digest, Sha256};
        let source = std::path::PathBuf::from(
            std::env::var_os("SHACRAFT_UPDATER_TEST_IMAGE").expect("old AppImage path"),
        );
        assert!(source.is_file());
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("isolated-old.AppImage");
        std::fs::copy(&source, &destination).unwrap();
        let old_hash = Sha256::digest(std::fs::read(&destination).unwrap());
        let config: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context
            .config_mut()
            .plugins
            .0
            .insert("updater".into(), config["plugins"]["updater"].clone());
        context.package_info_mut().version = "0.1.2".parse().unwrap();
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap();
        let key = public_key(app.handle()).unwrap();
        let builder = trusted_builder(app.handle())
            .unwrap()
            .executable_path(&destination);
        tauri::async_runtime::block_on(async {
            let update = check_candidate(builder, &key, "0.1.2", InstallationKind::Appimage)
                .await
                .unwrap()
                .expect("newer published version");
            let mut bytes = download_verified(&update, &key, |_, _| {}).await.unwrap();
            let new_hash = Sha256::digest(&bytes);
            assert_ne!(old_hash, new_hash);
            bytes[0] ^= 1;
            assert!(install_verified(&update, &bytes, &key, Some(&destination)).is_err());
            assert_eq!(
                old_hash,
                Sha256::digest(std::fs::read(&destination).unwrap())
            );
            bytes[0] ^= 1;
            install_verified(&update, &bytes, &key, Some(&destination)).unwrap();
            assert_eq!(
                new_hash,
                Sha256::digest(std::fs::read(&destination).unwrap())
            );
            assert_eq!(old_hash, Sha256::digest(std::fs::read(&source).unwrap()));
            println!(
                "Verified signed update {} and tamper rejection; replaced only isolated copy",
                update.version
            );
        });
    }
}
