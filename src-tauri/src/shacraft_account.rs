//! ShaCraft local-account client.
//!
//! The API origin is fixed in the binary. Passwords are sent only over HTTPS
//! and are never persisted; only the random, revocable session token is kept.

use reqwest::blocking::{Client, Response};
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};
use std::{
    fmt, fs,
    io::{self, Read},
    path::Path,
    time::Duration,
};

const API_ORIGIN: &str = "https://shacraft.ru";
const SESSION_FILE: &str = "shacraft-session";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AccountLink {
    pub server_id: String,
    pub mc_username: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Account {
    pub username: String,
    pub links: Vec<AccountLink>,
}

#[derive(Deserialize)]
struct AuthResponse {
    session_token: String,
    account: Account,
    recovery_codes: Vec<String>,
}

#[derive(Serialize)]
struct Credentials<'a> {
    username: &'a str,
    password: &'a str,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginResult {
    pub account: Account,
    pub recovery_codes: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct LinkStart {
    pub challenge_id: i64,
    pub expires_in_seconds: u64,
    pub registered_on_server: bool,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct LinkStatus {
    pub status: String,
    pub detail: Option<String>,
}

#[derive(Debug)]
pub enum AccountError {
    Network(reqwest::Error),
    Api(String),
    Io(io::Error),
    InvalidSession,
}

impl fmt::Display for AccountError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(error) => write!(formatter, "Нет связи с аккаунтами ShaCraft: {error}"),
            Self::Api(message) => formatter.write_str(message),
            Self::Io(error) => write!(formatter, "Не удалось сохранить сессию: {error}"),
            Self::InvalidSession => formatter.write_str("Сессия ShaCraft истекла — войдите снова"),
        }
    }
}

fn client() -> Result<Client, AccountError> {
    Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .redirect(Policy::none())
        .build()
        .map_err(AccountError::Network)
}

fn api_error(response: Response) -> AccountError {
    #[derive(Deserialize)]
    struct ErrorBody {
        detail: Option<String>,
    }
    let status = response.status();
    let detail = response
        .json::<ErrorBody>()
        .ok()
        .and_then(|body| body.detail);
    AccountError::Api(detail.unwrap_or_else(|| format!("ShaCraft API: HTTP {status}")))
}

fn session_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join(SESSION_FILE)
}

fn save_session(data_dir: &Path, token: &str) -> Result<(), AccountError> {
    fs::create_dir_all(data_dir).map_err(AccountError::Io)?;
    let path = session_path(data_dir);
    crate::storage::write_atomic(&path, token.as_bytes()).map_err(AccountError::Io)
}

fn load_session(data_dir: &Path) -> Result<String, AccountError> {
    let token = fs::read_to_string(session_path(data_dir)).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            AccountError::InvalidSession
        } else {
            AccountError::Io(error)
        }
    })?;
    let token = token.trim();
    if token.len() < 32 || token.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return Err(AccountError::InvalidSession);
    }
    Ok(token.to_owned())
}

pub fn authenticate(
    data_dir: &Path,
    username: &str,
    password: &str,
    register: bool,
) -> Result<LoginResult, AccountError> {
    let endpoint = if register {
        "/api/launcher/auth/register"
    } else {
        "/api/launcher/auth/login"
    };
    let response = client()?
        .post(format!("{API_ORIGIN}{endpoint}"))
        .json(&Credentials { username, password })
        .send()
        .map_err(AccountError::Network)?;
    if !response.status().is_success() {
        return Err(api_error(response));
    }
    let payload = response
        .json::<AuthResponse>()
        .map_err(AccountError::Network)?;
    save_session(data_dir, &payload.session_token)?;
    Ok(LoginResult {
        account: payload.account,
        recovery_codes: payload.recovery_codes,
    })
}

pub fn get_account(data_dir: &Path) -> Result<Account, AccountError> {
    let token = load_session(data_dir)?;
    let response = client()?
        .get(format!("{API_ORIGIN}/api/launcher/account"))
        .bearer_auth(token)
        .send()
        .map_err(AccountError::Network)?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        let _ = fs::remove_file(session_path(data_dir));
        return Err(AccountError::InvalidSession);
    }
    if !response.status().is_success() {
        return Err(api_error(response));
    }
    response.json::<Account>().map_err(AccountError::Network)
}

pub fn logout(data_dir: &Path) -> Result<(), AccountError> {
    if let Ok(token) = load_session(data_dir) {
        let _ = client()?
            .post(format!("{API_ORIGIN}/api/launcher/auth/logout"))
            .bearer_auth(token)
            .json(&serde_json::json!({}))
            .send();
    }
    match fs::remove_file(session_path(data_dir)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AccountError::Io(error)),
    }
}

pub fn start_link(
    data_dir: &Path,
    server_id: &str,
    nickname: &str,
) -> Result<LinkStart, AccountError> {
    let token = load_session(data_dir)?;
    let response = client()?
        .post(format!("{API_ORIGIN}/api/account/link/start"))
        .bearer_auth(token)
        .json(&serde_json::json!({"server_id": server_id, "mc_username": nickname}))
        .send()
        .map_err(AccountError::Network)?;
    if !response.status().is_success() {
        return Err(api_error(response));
    }
    response.json::<LinkStart>().map_err(AccountError::Network)
}

pub fn link_status(data_dir: &Path, challenge_id: i64) -> Result<LinkStatus, AccountError> {
    let token = load_session(data_dir)?;
    let response = client()?
        .get(format!(
            "{API_ORIGIN}/api/account/link/status/{challenge_id}"
        ))
        .bearer_auth(token)
        .send()
        .map_err(AccountError::Network)?;
    if !response.status().is_success() {
        return Err(api_error(response));
    }
    response.json::<LinkStatus>().map_err(AccountError::Network)
}

const ADMISSION_ENDPOINT: &str = "/api/launcher/v2/admission/tickets";

/// Error responses at this boundary never echo arbitrary response bodies: a
/// misconfigured proxy/service must not copy credentials into UI diagnostics.
fn admission_error(status: reqwest::StatusCode) -> AccountError {
    use reqwest::StatusCode;
    match status {
        StatusCode::UNAUTHORIZED => AccountError::InvalidSession,
        StatusCode::FORBIDDEN => AccountError::Api(
            "Нет разрешения на вход в Aeronautics. Проверьте привязку ника и доступ к серверу в аккаунте ShaCraft.".into()),
        StatusCode::CONFLICT => AccountError::Api(
            "Этот ник уже занят или зарезервирован. Если это ваш игровой ник, обратитесь в поддержку ShaCraft.".into()),
        StatusCode::NOT_FOUND | StatusCode::SERVICE_UNAVAILABLE => AccountError::Api(
            "Вход через ShaCraft Launcher пока не настроен на сервере. Повторите попытку позже.".into()),
        StatusCode::TOO_MANY_REQUESTS => AccountError::Api(
            "Слишком много запросов входа. Подождите немного и повторите попытку.".into()),
        _ => AccountError::Api(format!("Не удалось получить разрешение ShaCraft: HTTP {status}")),
    }
}

fn checked_admission_response(
    data_dir: &Path,
    response: Response,
) -> Result<Response, AccountError> {
    if response.status().is_success() {
        return Ok(response);
    }
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        let _ = fs::remove_file(session_path(data_dir));
    }
    Err(admission_error(response.status()))
}

/// The server reserves a free nickname atomically for this account. Existing
/// player names remain reserved for administrator-assisted migration.
pub fn claim_nickname(data_dir: &Path, nickname: &str) -> Result<Account, AccountError> {
    let token = load_session(data_dir)?;
    let response = client()?
        .post(format!("{API_ORIGIN}/api/launcher/v2/admission/nickname"))
        .bearer_auth(token)
        .json(&serde_json::json!({"server_id": "aoc", "mc_username": nickname}))
        .send()
        .map_err(AccountError::Network)?;
    checked_admission_response(data_dir, response)?
        .json()
        .map_err(AccountError::Network)
}

/// Called only after installation, immediately before Java spawn. Nothing in
/// this response is exposed to the webview or persisted with account settings.
pub(crate) fn issue_admission(
    data_dir: &Path,
) -> Result<crate::admission::Admission, AccountError> {
    let token = load_session(data_dir)?;
    let key = crate::admission::AdmissionKey::generate()
        .map_err(|message| AccountError::Api(message.into()))?;
    let response = client()?
        .post(format!("{API_ORIGIN}{ADMISSION_ENDPOINT}"))
        .bearer_auth(token)
        .json(&key.request())
        .send()
        .map_err(AccountError::Network)?;
    let response = checked_admission_response(data_dir, response)?;
    // The expected object is under 256 bytes; bound the remote allocation and
    // use a fixed parse error without response values or secret-bearing bodies.
    let mut body = zeroize::Zeroizing::new(Vec::new());
    response.take(4097).read_to_end(&mut body).map_err(|_| {
        AccountError::Api(
            "Не удалось прочитать разрешение на вход. Повторите попытку позже.".into(),
        )
    })?;
    let invalid = || {
        AccountError::Api(
            "Сервер вернул некорректное разрешение на вход. Повторите попытку позже.".into(),
        )
    };
    if body.len() > 4096 {
        return Err(invalid());
    }
    let payload = serde_json::from_slice(&body).map_err(|_| invalid())?;
    key.bind(payload)
        .map_err(|message| AccountError::Api(message.into()))
}

#[cfg(test)]
mod tests {
    use super::{
        admission_error, issue_admission, load_session, save_session, session_path, AccountError,
    };
    use std::{
        fs, process,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temporary_directory() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "shacraft-account-test-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn session_round_trips_without_password_storage() {
        let directory = temporary_directory();
        let token = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG";
        save_session(&directory, token).unwrap();
        assert_eq!(load_session(&directory).unwrap(), token);
        assert_eq!(fs::read_to_string(session_path(&directory)).unwrap(), token);
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn session_is_private_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let directory = temporary_directory();
        save_session(&directory, "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG").unwrap();
        assert_eq!(
            fs::metadata(session_path(&directory))
                .unwrap()
                .permissions()
                .mode()
                & 0o077,
            0
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn admission_without_session_never_falls_back_to_legacy_nickname() {
        let directory = temporary_directory();
        crate::settings::save(&directory, crate::settings::LauncherSettings::default()).unwrap();
        assert!(matches!(
            issue_admission(&directory),
            Err(AccountError::InvalidSession)
        ));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn admission_unavailable_and_revoked_session_have_actionable_errors() {
        for status in [
            reqwest::StatusCode::NOT_FOUND,
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
        ] {
            assert!(admission_error(status)
                .to_string()
                .contains("пока не настроен"));
        }
        assert!(matches!(
            admission_error(reqwest::StatusCode::UNAUTHORIZED),
            AccountError::InvalidSession
        ));
        assert!(admission_error(reqwest::StatusCode::CONFLICT)
            .to_string()
            .contains("зарезервирован"));
    }
}
