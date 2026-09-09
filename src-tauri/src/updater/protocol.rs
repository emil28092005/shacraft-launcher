//! Signed launcher releases are a separate trust domain from the ShaCraft pack.
//! Authenticate the exact metadata bytes before parsing any URLs or versions.
use base64::{engine::general_purpose::STANDARD, Engine};
use minisign_verify::{PublicKey, Signature};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Read};
use url::Url;

pub(crate) const RELEASES: &str = "https://github.com/emil28092005/shacraft-launcher/releases";
pub(crate) const LATEST: &str =
    "https://github.com/emil28092005/shacraft-launcher/releases/latest/download/latest.json";
pub(crate) const MAX_METADATA: u64 = 32768;
pub(crate) const MAX_SIGNATURE: u64 = 2048;
pub(crate) const MAX_PACKAGE: u64 = 1024 * 1024 * 1024;
const PINNED_KEY: &str = include_str!("../../updater-public-key.txt");
pub(crate) fn test_build() -> bool {
    option_env!("SHACRAFT_UPDATER_TEST_BUILD") == Some("1")
}

pub(crate) fn configured_key() -> Option<&'static str> {
    let injected = option_env!("SHACRAFT_UPDATER_PUBLIC_KEY").map(str::trim);
    let pinned = PINNED_KEY.trim();
    // Explicit CI-only builds may use a disposable real signing key. Release CI
    // forbids this switch; every such binary exposes its test provenance in status.
    if test_build() {
        return injected.filter(|key| key_is_valid(key));
    }
    if key_is_valid(pinned) && injected.is_none_or(|key| key == pinned) {
        Some(pinned)
    } else {
        None
    }
}

const PLATFORMS: [(&str, &str); 4] = [
    ("windows-x86_64", "windows-x86_64-setup.exe"),
    ("linux-x86_64", "linux-x86_64.AppImage"),
    ("darwin-aarch64", "darwin-aarch64.app.tar.gz"),
    ("darwin-x86_64", "darwin-x86_64.app.tar.gz"),
];
const MANUAL: [(&str, &str); 4] = [
    ("windows-x86_64-msi", "windows-x86_64.msi"),
    ("linux-x86_64-deb", "linux-x86_64.deb"),
    ("darwin-aarch64-dmg", "darwin-aarch64.dmg"),
    ("darwin-x86_64-dmg", "darwin-x86_64.dmg"),
];

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Artifact {
    pub url: String,
    pub signature: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Release {
    pub schema_version: u32,
    pub version: String,
    pub tag: String,
    pub notes: String,
    #[serde(rename = "pub_date")]
    pub pub_date: String,
    pub platforms: BTreeMap<String, Artifact>,
    pub manual_packages: BTreeMap<String, Artifact>,
}

#[derive(Clone)]
pub(crate) struct VerifiedRelease {
    pub release: Release,
    pub json: serde_json::Value,
}

fn decode_text(encoded: &str) -> Result<String, String> {
    let bytes = STANDARD
        .decode(encoded.trim())
        .map_err(|_| "Некорректный формат подписи обновления")?;
    String::from_utf8(bytes).map_err(|_| "Некорректный формат подписи обновления".into())
}

pub(crate) fn key_is_valid(key: &str) -> bool {
    key.len() <= MAX_SIGNATURE as usize
        && decode_text(key)
            .ok()
            .and_then(|text| PublicKey::decode(&text).ok())
            .is_some()
}

pub(crate) fn verify_signature(bytes: &[u8], signature: &str, key: &str) -> Result<(), String> {
    if signature.len() > MAX_SIGNATURE as usize || key.len() > MAX_SIGNATURE as usize {
        return Err("Некорректный размер подписи обновления".into());
    }
    let public = PublicKey::decode(&decode_text(key)?)
        .map_err(|_| "Не настроен доверенный ключ обновлений")?;
    let signature = Signature::decode(&decode_text(signature)?)
        .map_err(|_| "Некорректный формат подписи обновления")?;
    // Same minisign primitive/legacy compatibility as tauri-plugin-updater 2.11.0.
    public
        .verify(bytes, &signature, true)
        .map_err(|_| "Подпись обновления не прошла проверку".into())
}

fn stable_version(value: &str) -> Result<Version, String> {
    let v = Version::parse(value).map_err(|_| "Некорректная версия обновления")?;
    if !v.pre.is_empty()
        || !v.build.is_empty()
        || v.to_string() != value
        || v.major > 255
        || v.minor > 255
        || v.patch > 65535
    {
        return Err("Разрешены только стабильные версии обновлений".into());
    }
    Ok(v)
}

impl VerifiedRelease {
    pub fn parse(bytes: &[u8], signature: &str, key: &str) -> Result<Self, String> {
        if bytes.len() as u64 > MAX_METADATA {
            return Err("Слишком большой список обновлений".into());
        }
        verify_signature(bytes, signature, key)?;
        let release: Release = serde_json::from_slice(bytes)
            .map_err(|_| "Некорректные подписанные метаданные обновления")?;
        stable_version(&release.version)?;
        if release.schema_version != 1
            || release.tag != format!("v{}", release.version)
            || release.notes.len() > 4096
            || release.pub_date.len() != 20
            || !release.pub_date.ends_with('Z')
        {
            return Err("Неподдерживаемые метаданные обновления".into());
        }
        let date = time::OffsetDateTime::parse(
            &release.pub_date,
            &time::format_description::well_known::Rfc3339,
        )
        .map_err(|_| "Некорректная дата подписанного выпуска")?;
        if date
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|_| "Некорректная дата выпуска")?
            != release.pub_date
        {
            return Err("Некорректная дата подписанного выпуска".into());
        }
        validate_artifacts(&release.platforms, &PLATFORMS, &release)?;
        validate_artifacts(&release.manual_packages, &MANUAL, &release)?;
        Ok(Self {
            release,
            json: serde_json::from_slice(bytes).map_err(|_| "Некорректные метаданные")?,
        })
    }

    pub fn is_newer_than(&self, current: &str) -> Result<bool, String> {
        let available = stable_version(&self.release.version)?;
        let installed = stable_version(current)?;
        if available < installed {
            return Err(
                "Сервер предложил более старую версию. Понижение версии заблокировано.".into(),
            );
        }
        Ok(available > installed)
    }

    pub fn pinned_endpoint(&self) -> String {
        format!("{RELEASES}/download/{}/latest.json", self.release.tag)
    }
}

fn validate_artifacts(
    values: &BTreeMap<String, Artifact>,
    expected: &[(&str, &str)],
    release: &Release,
) -> Result<(), String> {
    if values.len() != expected.len() {
        return Err("Неполный список платформ обновления".into());
    }
    for (platform, suffix) in expected {
        let artifact = values
            .get(*platform)
            .ok_or("Отсутствует ожидаемая платформа обновления")?;
        let expected_url = format!(
            "{RELEASES}/download/{}/shacraft-launcher_{}_{}",
            release.tag, release.version, suffix
        );
        if artifact.url != expected_url
            || artifact.size == 0
            || artifact.size > MAX_PACKAGE
            || artifact.sha256.len() != 64
            || !artifact
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || artifact.signature.is_empty()
            || artifact.signature.len() > MAX_SIGNATURE as usize
        {
            return Err("Неверная привязка пакета к версии, платформе или репозиторию".into());
        }
        let text = decode_text(&artifact.signature)?;
        Signature::decode(&text).map_err(|_| "Некорректная подпись пакета обновления")?;
    }
    Ok(())
}

/// Only these GitHub release hosts may participate in HTTPS redirects. CDN query
/// parameters are GitHub's signed delivery URLs; initial URLs are exact literals.
pub(crate) fn redirect_allowed(url: &Url) -> bool {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || url.fragment().is_some()
    {
        return false;
    }
    match url.host_str() {
        Some("github.com") => {
            url.query().is_none()
                && url
                    .path()
                    .starts_with("/emil28092005/shacraft-launcher/releases/")
        }
        Some("release-assets.githubusercontent.com") => true,
        _ => false,
    }
}

pub(crate) fn read_bounded(
    mut reader: impl Read,
    limit: u64,
    mut progress: impl FnMut(u64),
) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|_| "Соединение прервалось. Повторите загрузку обновления.")?;
        if count == 0 {
            break;
        }
        if result.len() as u64 + count as u64 > limit {
            return Err("Размер ответа превышает подписанный предел".into());
        }
        result.extend_from_slice(&buffer[..count]);
        progress(result.len() as u64);
    }
    Ok(result)
}

pub(crate) fn verify_artifact(bytes: &[u8], artifact: &Artifact, key: &str) -> Result<(), String> {
    if bytes.len() as u64 != artifact.size
        || format!("{:x}", Sha256::digest(bytes)) != artifact.sha256
    {
        return Err(
            "Размер или SHA-256 пакета обновления не совпал. Установка остановлена.".into(),
        );
    }
    verify_signature(bytes, &artifact.signature, key)
}

pub(crate) fn install_verified<T>(
    bytes: &[u8],
    artifact: &Artifact,
    key: &str,
    installer: impl FnOnce(&[u8]) -> Result<T, String>,
) -> Result<T, String> {
    verify_artifact(bytes, artifact, key)?;
    installer(bytes)
}

/// A narrow injectable read boundary; production uses only the fixed HTTPS client.
pub(crate) fn fetch_release(
    mut fetch: impl FnMut(&str, u64) -> Result<Option<Vec<u8>>, String>,
    key: &str,
    current: &str,
) -> Result<Option<VerifiedRelease>, String> {
    let Some(bytes) = fetch(LATEST, MAX_METADATA)? else {
        return Ok(None);
    };
    let signature = fetch(&format!("{LATEST}.sig"), MAX_SIGNATURE)?
        .ok_or("Отсутствует подпись списка обновлений")?;
    let signature =
        std::str::from_utf8(&signature).map_err(|_| "Некорректная подпись списка обновлений")?;
    let verified = VerifiedRelease::parse(&bytes, signature, key)?;
    Ok(verified.is_newer_than(current)?.then_some(verified))
}

#[cfg(test)]
mod tests;
