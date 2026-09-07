//! ShaCraft local-account client.
//!
//! The API origin is fixed in the binary. Passwords are sent only over HTTPS
//! and are never persisted; only the random, revocable session token is kept.

use reqwest::blocking::{Client, Response};
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};
use std::{fmt, fs, io, path::Path, time::Duration};

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
    NoLinkedNickname,
}

impl fmt::Display for AccountError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(error) => write!(formatter, "Нет связи с аккаунтами ShaCraft: {error}"),
            Self::Api(message) => formatter.write_str(message),
            Self::Io(error) => write!(formatter, "Не удалось сохранить сессию: {error}"),
            Self::InvalidSession => formatter.write_str("Сессия ShaCraft истекла — войдите снова"),
            Self::NoLinkedNickname => formatter.write_str("Сначала привяжите игровой ник к серверу Aeronautics"),
        }
    }
}

fn client() -> Result<Client, AccountError> {
    Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(Policy::none())
        .build()
        .map_err(AccountError::Network)
}

fn api_error(response: Response) -> AccountError {
    #[derive(Deserialize)]
    struct ErrorBody { detail: Option<String> }
    let status = response.status();
    let detail = response.json::<ErrorBody>().ok().and_then(|body| body.detail);
    AccountError::Api(detail.unwrap_or_else(|| format!("ShaCraft API: HTTP {status}")))
}

fn session_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join(SESSION_FILE)
}

fn save_session(data_dir: &Path, token: &str) -> Result<(), AccountError> {
    fs::create_dir_all(data_dir).map_err(AccountError::Io)?;
    let path = session_path(data_dir);
    let temporary = data_dir.join(".shacraft-session.part");
    fs::write(&temporary, token.as_bytes()).map_err(AccountError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600)).map_err(AccountError::Io)?;
    }
    fs::rename(temporary, path).map_err(AccountError::Io)
}

fn load_session(data_dir: &Path) -> Result<String, AccountError> {
    let token = fs::read_to_string(session_path(data_dir)).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound { AccountError::InvalidSession } else { AccountError::Io(error) }
    })?;
    let token = token.trim();
    if token.len() < 32 || token.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return Err(AccountError::InvalidSession);
    }
    Ok(token.to_owned())
}

pub fn authenticate(data_dir: &Path, username: &str, password: &str, register: bool) -> Result<LoginResult, AccountError> {
    let endpoint = if register { "/api/launcher/auth/register" } else { "/api/launcher/auth/login" };
    let response = client()?.post(format!("{API_ORIGIN}{endpoint}"))
        .json(&Credentials { username, password }).send().map_err(AccountError::Network)?;
    if !response.status().is_success() { return Err(api_error(response)); }
    let payload = response.json::<AuthResponse>().map_err(AccountError::Network)?;
    save_session(data_dir, &payload.session_token)?;
    Ok(LoginResult { account: payload.account, recovery_codes: payload.recovery_codes })
}

pub fn get_account(data_dir: &Path) -> Result<Account, AccountError> {
    let token = load_session(data_dir)?;
    let response = client()?.get(format!("{API_ORIGIN}/api/launcher/account"))
        .bearer_auth(token).send().map_err(AccountError::Network)?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        let _ = fs::remove_file(session_path(data_dir));
        return Err(AccountError::InvalidSession);
    }
    if !response.status().is_success() { return Err(api_error(response)); }
    response.json::<Account>().map_err(AccountError::Network)
}

pub fn logout(data_dir: &Path) -> Result<(), AccountError> {
    if let Ok(token) = load_session(data_dir) {
        let _ = client()?.post(format!("{API_ORIGIN}/api/launcher/auth/logout"))
            .bearer_auth(token).json(&serde_json::json!({})).send();
    }
    match fs::remove_file(session_path(data_dir)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AccountError::Io(error)),
    }
}

pub fn start_link(data_dir: &Path, server_id: &str, nickname: &str) -> Result<LinkStart, AccountError> {
    let token = load_session(data_dir)?;
    let response = client()?.post(format!("{API_ORIGIN}/api/account/link/start"))
        .bearer_auth(token).json(&serde_json::json!({"server_id": server_id, "mc_username": nickname}))
        .send().map_err(AccountError::Network)?;
    if !response.status().is_success() { return Err(api_error(response)); }
    response.json::<LinkStart>().map_err(AccountError::Network)
}

pub fn link_status(data_dir: &Path, challenge_id: i64) -> Result<LinkStatus, AccountError> {
    let token = load_session(data_dir)?;
    let response = client()?.get(format!("{API_ORIGIN}/api/account/link/status/{challenge_id}"))
        .bearer_auth(token).send().map_err(AccountError::Network)?;
    if !response.status().is_success() { return Err(api_error(response)); }
    response.json::<LinkStatus>().map_err(AccountError::Network)
}

pub fn aeronautics_nickname(data_dir: &Path) -> Result<String, AccountError> {
    get_account(data_dir)?.links.into_iter()
        .find(|link| link.server_id == "aoc")
        .map(|link| link.mc_username)
        .ok_or(AccountError::NoLinkedNickname)
}

#[cfg(test)]
mod tests {
    use super::{load_session, save_session, session_path};
    use std::{fs, process, time::{SystemTime, UNIX_EPOCH}};

    fn temporary_directory() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "shacraft-account-test-{}-{}",
            process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
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
        assert_eq!(fs::metadata(session_path(&directory)).unwrap().permissions().mode() & 0o077, 0);
        fs::remove_dir_all(directory).unwrap();
    }
}
