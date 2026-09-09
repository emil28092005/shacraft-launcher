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
    pub proof_version: u32,
    pub server_id: String,
    pub challenge_id: i64,
    pub expires_in_seconds: u64,
    pub registered_on_server: bool,
    pub proof_code: String,
    pub mc_username: String,
    pub player_uuid: String,
}

#[derive(Deserialize)]
pub struct OnboardingGrant {
    #[serde(flatten)]
    pub challenge: LinkStart,
    pub grant_token: String,
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
            Self::NoLinkedNickname => {
                formatter.write_str("Сначала привяжите игровой ник к серверу Aeronautics")
            }
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
    let value = response
        .json::<LinkStart>()
        .map_err(AccountError::Network)?;
    validate_challenge(&value, server_id, nickname)?;
    Ok(value)
}

pub fn valid_nickname(name: &str) -> bool {
    (3..=16).contains(&name.len()) && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

fn validate_challenge(
    value: &LinkStart,
    server_id: &str,
    requested: &str,
) -> Result<(), AccountError> {
    if value.proof_version != 1
        || value.server_id != server_id
        || !valid_nickname(requested)
        || value.mc_username != requested
        || value.player_uuid != crate::session::offline_uuid(requested)
        || value.challenge_id <= 0
        || value.expires_in_seconds == 0
        || value.expires_in_seconds > 600
        || value.proof_code.len() != 32
        || !value.proof_code.bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err(AccountError::Api(
            "Сервер вернул неподходящее подтверждение ника".into(),
        ));
    }
    Ok(())
}

pub fn start_onboarding(data_dir: &Path, nickname: &str) -> Result<OnboardingGrant, AccountError> {
    if !valid_nickname(nickname) {
        return Err(AccountError::Api("Неверный игровой ник".into()));
    }
    let response = client()?
        .post(format!("{API_ORIGIN}/api/launcher/onboarding/start"))
        .bearer_auth(load_session(data_dir)?)
        .json(&serde_json::json!({"server_id":"aoc","mc_username":nickname}))
        .send()
        .map_err(AccountError::Network)?;
    if !response.status().is_success() {
        return Err(api_error(response));
    }
    let grant: OnboardingGrant = response.json().map_err(AccountError::Network)?;
    validate_challenge(&grant.challenge, "aoc", nickname)?;
    if grant.grant_token.len() < 32
        || grant.grant_token.len() > 256
        || !grant
            .grant_token
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err(AccountError::Api(
            "Некорректное разрешение первого входа".into(),
        ));
    }
    Ok(grant)
}

pub fn validate_onboarding(data_dir: &Path, grant: &OnboardingGrant) -> Result<(), AccountError> {
    let response = client()?.post(format!("{API_ORIGIN}/api/launcher/onboarding/validate"))
        .bearer_auth(load_session(data_dir)?)
        .json(&serde_json::json!({"challenge_id":grant.challenge.challenge_id,"grant_token":grant.grant_token}))
        .send().map_err(AccountError::Network)?;
    if !response.status().is_success() {
        return Err(api_error(response));
    }
    let data: serde_json::Value = response.json().map_err(AccountError::Network)?;
    if data["server_id"] != "aoc"
        || data["mc_username"] != grant.challenge.mc_username
        || data["player_uuid"] != grant.challenge.player_uuid
        || data["challenge_id"] != grant.challenge.challenge_id
        || data["proof_version"] != 1
        || !data["expires_in_seconds"]
            .as_u64()
            .is_some_and(|n| n > 0 && n <= 600)
    {
        return Err(AccountError::Api(
            "Первый вход не подтверждён сервером".into(),
        ));
    }
    Ok(())
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

pub fn aeronautics_nickname(data_dir: &Path) -> Result<String, AccountError> {
    get_account(data_dir)?
        .links
        .into_iter()
        .find(|link| link.server_id == "aoc")
        .map(|link| link.mc_username)
        .ok_or(AccountError::NoLinkedNickname)
}

#[cfg(test)]
mod tests {
    use super::{load_session, save_session, session_path};
    use std::{
        fs, process,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn challenge_requires_matching_server_exact_nickname_uuid_and_bounded_nonce() {
        let valid = super::LinkStart {
            proof_version: 1,
            server_id: "aoc".into(),
            challenge_id: 1,
            expires_in_seconds: 600,
            registered_on_server: false,
            proof_code: "a".repeat(32),
            mc_username: "ShaCraft_Test".into(),
            player_uuid: crate::session::offline_uuid("ShaCraft_Test"),
        };
        assert!(super::validate_challenge(&valid, "aoc", "ShaCraft_Test").is_ok());
        assert!(super::validate_challenge(&valid, "create", "ShaCraft_Test").is_err());
        assert!(super::validate_challenge(&valid, "aoc", "shacraft_test").is_err());
        let mut wrong = valid.clone();
        wrong.player_uuid = crate::session::offline_uuid("shacraft_test");
        assert!(super::validate_challenge(&wrong, "aoc", "ShaCraft_Test").is_err());
        for ttl in [0, 601] {
            wrong = valid.clone();
            wrong.expires_in_seconds = ttl;
            assert!(super::validate_challenge(&wrong, "aoc", "ShaCraft_Test").is_err());
        }
        wrong = valid.clone();
        wrong.proof_version = 0;
        assert!(super::validate_challenge(&wrong, "aoc", "ShaCraft_Test").is_err());
        for code in ["a".repeat(31), "g".repeat(32), "a".repeat(33)] {
            wrong = valid.clone();
            wrong.proof_code = code;
            assert!(super::validate_challenge(&wrong, "aoc", "ShaCraft_Test").is_err());
        }
    }

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
}
