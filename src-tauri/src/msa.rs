//! Real Microsoft account login: device-code OAuth -> Xbox Live -> XSTS ->
//! Minecraft Services -> game-ownership check. This is what makes the
//! launcher only playable by people who actually own Minecraft Java
//! Edition; nothing here is optional or bypassable by a manifest.
//!
//! CORRECTION (2026-09-06): an earlier version of this module assumed a
//! public, no-registration-needed client ID existed for this flow. That
//! was wrong — verified live against `login.microsoftonline.com`, which
//! rejects it (`AADSTS700016`, app not found). Microsoft requires every
//! app to have its own Azure AD (Entra ID) "public client" registration
//! (no client secret needed for the device code grant — see
//! <https://aka.ms/AppRegistrations>), *and* new registrations must be
//! separately approved for Minecraft/Xbox API access via
//! <https://aka.ms/mce-reviewappid> before Xbox Live/Minecraft Services
//! will accept their tokens. `MSA_CLIENT_ID` below is a placeholder until
//! ShaCraft completes that registration; `start_device_code` refuses to
//! run while it's still the placeholder rather than fail confusingly
//! against Microsoft. Every endpoint below is otherwise a hardcoded HTTPS
//! constant, matching the trust-domain pattern used for Mojang/NeoForge
//! elsewhere in this crate — only the client ID is deployment-specific.

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::{
    fmt, fs, io,
    path::Path,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// ShaCraft's own Azure AD application (client) ID, registered as a public
/// client with device-code flow allowed and approved for Minecraft API
/// access. Replace this before shipping login — see the module doc above.
const MSA_CLIENT_ID: &str = "00000000-0000-0000-0000-000000000000";

pub fn is_configured() -> bool {
    MSA_CLIENT_ID != "00000000-0000-0000-0000-000000000000"
}

const DEVICE_CODE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const XBOX_USER_AUTH_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_AUTHORIZE_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MINECRAFT_LOGIN_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MINECRAFT_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";
const ACCOUNT_FILE: &str = "account.json";

#[derive(Debug)]
pub enum MsaError {
    NotConfigured,
    Network(reqwest::Error),
    HttpStatus(reqwest::StatusCode),
    AuthorizationDeclined,
    AuthorizationExpired,
    NoXboxAccount,
    DoesNotOwnMinecraft,
    Io(io::Error),
    UnexpectedResponse(String),
}

impl fmt::Display for MsaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConfigured => formatter.write_str(
                "ShaCraft has not configured Microsoft login yet (MSA_CLIENT_ID is a placeholder) — \
                 register an Azure AD app at https://aka.ms/AppRegistrations and get it approved for \
                 the Minecraft API at https://aka.ms/mce-reviewappid, then set MSA_CLIENT_ID in msa.rs",
            ),
            Self::Network(error) => write!(formatter, "network error: {error}"),
            Self::HttpStatus(status) => write!(formatter, "unexpected response: {status}"),
            Self::AuthorizationDeclined => formatter.write_str("Login was declined"),
            Self::AuthorizationExpired => formatter.write_str("Login code expired before it was used"),
            Self::NoXboxAccount => formatter.write_str("This Microsoft account has no Xbox profile"),
            Self::DoesNotOwnMinecraft => formatter.write_str("This Microsoft account does not own Minecraft: Java Edition"),
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::UnexpectedResponse(message) => write!(formatter, "unexpected response: {message}"),
        }
    }
}

impl From<io::Error> for MsaError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

// ---------------------------------------------------------------------
// Device code flow
// ---------------------------------------------------------------------

pub struct DeviceCodeStart {
    pub verification_uri: String,
    pub user_code: String,
    pub expires_in_seconds: u64,
    device_code: String,
    interval_seconds: u64,
}

#[derive(Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}

pub fn start_device_code(client: &Client) -> Result<DeviceCodeStart, MsaError> {
    if !is_configured() {
        return Err(MsaError::NotConfigured);
    }
    let response = client
        .post(DEVICE_CODE_URL)
        .form(&[("client_id", MSA_CLIENT_ID), ("scope", "XboxLive.signin offline_access")])
        .send()
        .map_err(MsaError::Network)?;
    if !response.status().is_success() {
        return Err(MsaError::HttpStatus(response.status()));
    }
    let body: DeviceCodeResponse = response.json().map_err(MsaError::Network)?;
    Ok(DeviceCodeStart {
        verification_uri: body.verification_uri,
        user_code: body.user_code,
        expires_in_seconds: body.expires_in,
        device_code: body.device_code,
        interval_seconds: body.interval.max(5),
    })
}

pub struct MicrosoftTokens {
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    error: Option<String>,
}

/// Blocks, polling on `start.interval_seconds`, until the user finishes
/// signing in at `start.verification_uri`, the code expires, or they
/// decline. This is the slow step in the whole login flow — the caller
/// should already have shown `verification_uri`/`user_code` to the user
/// before calling this (see `start_device_code`).
pub fn poll_device_code(client: &Client, start: &DeviceCodeStart) -> Result<MicrosoftTokens, MsaError> {
    let deadline = Instant::now() + Duration::from_secs(start.expires_in_seconds);
    let mut interval = Duration::from_secs(start.interval_seconds);

    loop {
        if Instant::now() >= deadline {
            return Err(MsaError::AuthorizationExpired);
        }
        thread::sleep(interval);

        let response = client
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", MSA_CLIENT_ID),
                ("device_code", &start.device_code),
            ])
            .send()
            .map_err(MsaError::Network)?;
        let status = response.status();
        let body: TokenResponse = response.json().map_err(MsaError::Network)?;

        if status.is_success() {
            let (Some(access_token), Some(refresh_token)) = (body.access_token, body.refresh_token) else {
                return Err(MsaError::UnexpectedResponse("token response missing access_token/refresh_token".into()));
            };
            return Ok(MicrosoftTokens { access_token, refresh_token });
        }

        match body.error.as_deref() {
            Some("authorization_pending") => continue,
            Some("slow_down") => {
                interval += Duration::from_secs(5);
                continue;
            }
            Some("authorization_declined") => return Err(MsaError::AuthorizationDeclined),
            Some("expired_token") => return Err(MsaError::AuthorizationExpired),
            other => return Err(MsaError::UnexpectedResponse(other.unwrap_or("unknown device code error").into())),
        }
    }
}

pub fn refresh_microsoft_tokens(client: &Client, refresh_token: &str) -> Result<MicrosoftTokens, MsaError> {
    if !is_configured() {
        return Err(MsaError::NotConfigured);
    }
    let response = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", MSA_CLIENT_ID),
            ("refresh_token", refresh_token),
            ("scope", "XboxLive.signin offline_access"),
        ])
        .send()
        .map_err(MsaError::Network)?;
    if !response.status().is_success() {
        return Err(MsaError::HttpStatus(response.status()));
    }
    let body: TokenResponse = response.json().map_err(MsaError::Network)?;
    let (Some(access_token), Some(refresh_token)) = (body.access_token, body.refresh_token) else {
        return Err(MsaError::UnexpectedResponse("refresh response missing access_token/refresh_token".into()));
    };
    Ok(MicrosoftTokens { access_token, refresh_token })
}

// ---------------------------------------------------------------------
// Xbox Live -> XSTS -> Minecraft Services
// ---------------------------------------------------------------------

#[derive(Serialize)]
struct XboxUserAuthRequest<'a> {
    #[serde(rename = "Properties")]
    properties: XboxUserAuthProperties<'a>,
    #[serde(rename = "RelyingParty")]
    relying_party: &'a str,
    #[serde(rename = "TokenType")]
    token_type: &'a str,
}

#[derive(Serialize)]
struct XboxUserAuthProperties<'a> {
    #[serde(rename = "AuthMethod")]
    auth_method: &'a str,
    #[serde(rename = "SiteName")]
    site_name: &'a str,
    #[serde(rename = "RpsTicket")]
    rps_ticket: String,
}

#[derive(Serialize)]
struct XstsRequest<'a> {
    #[serde(rename = "Properties")]
    properties: XstsProperties<'a>,
    #[serde(rename = "RelyingParty")]
    relying_party: &'a str,
    #[serde(rename = "TokenType")]
    token_type: &'a str,
}

#[derive(Serialize)]
struct XstsProperties<'a> {
    #[serde(rename = "SandboxId")]
    sandbox_id: &'a str,
    #[serde(rename = "UserTokens")]
    user_tokens: [&'a str; 1],
}

#[derive(Deserialize)]
struct XboxTokenResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: XboxDisplayClaims,
}

#[derive(Deserialize)]
struct XboxDisplayClaims {
    xui: Vec<XboxUserHash>,
}

#[derive(Deserialize)]
struct XboxUserHash {
    uhs: String,
    /// Xbox User ID, used for the game's `${auth_xuid}` launch argument.
    /// Absent for some account states; not required to play.
    #[serde(default)]
    xid: Option<String>,
}

fn xbox_live_user_token(client: &Client, microsoft_access_token: &str) -> Result<(String, String), MsaError> {
    let request = XboxUserAuthRequest {
        properties: XboxUserAuthProperties {
            auth_method: "RPS",
            site_name: "user.auth.xboxlive.com",
            rps_ticket: format!("d={microsoft_access_token}"),
        },
        relying_party: "http://auth.xboxlive.com",
        token_type: "JWT",
    };
    let response = client.post(XBOX_USER_AUTH_URL).json(&request).send().map_err(MsaError::Network)?;
    if !response.status().is_success() {
        return Err(MsaError::HttpStatus(response.status()));
    }
    let body: XboxTokenResponse = response.json().map_err(MsaError::Network)?;
    let uhs = body.display_claims.xui.into_iter().next().map(|claim| claim.uhs).ok_or_else(|| MsaError::UnexpectedResponse("missing uhs".into()))?;
    Ok((body.token, uhs))
}

fn xsts_authorize(client: &Client, xbox_live_token: &str) -> Result<(String, String, Option<String>), MsaError> {
    let request = XstsRequest {
        properties: XstsProperties { sandbox_id: "RETAIL", user_tokens: [xbox_live_token] },
        relying_party: "rp://api.minecraftservices.com/",
        token_type: "JWT",
    };
    let response = client.post(XSTS_AUTHORIZE_URL).json(&request).send().map_err(MsaError::Network)?;
    let status = response.status();
    if status.as_u16() == 401 {
        // XErr 2148916233 means the account has no Xbox profile at all
        // (common for brand-new Microsoft accounts); other 401 causes
        // (family/child accounts, regional restrictions) surface the same
        // way for now, kept as one clear error rather than guessing.
        return Err(MsaError::NoXboxAccount);
    }
    if !status.is_success() {
        return Err(MsaError::HttpStatus(status));
    }
    let body: XboxTokenResponse = response.json().map_err(MsaError::Network)?;
    let claim = body.display_claims.xui.into_iter().next().ok_or_else(|| MsaError::UnexpectedResponse("missing uhs".into()))?;
    Ok((body.token, claim.uhs, claim.xid))
}

#[derive(Serialize)]
struct MinecraftLoginRequest {
    #[serde(rename = "identityToken")]
    identity_token: String,
}

#[derive(Deserialize)]
struct MinecraftLoginResponse {
    access_token: String,
}

fn minecraft_login(client: &Client, user_hash: &str, xsts_token: &str) -> Result<String, MsaError> {
    let request = MinecraftLoginRequest { identity_token: format!("XBL3.0 x={user_hash};{xsts_token}") };
    let response = client.post(MINECRAFT_LOGIN_URL).json(&request).send().map_err(MsaError::Network)?;
    if !response.status().is_success() {
        return Err(MsaError::HttpStatus(response.status()));
    }
    let body: MinecraftLoginResponse = response.json().map_err(MsaError::Network)?;
    Ok(body.access_token)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinecraftProfile {
    pub id: String,
    pub name: String,
}

/// Confirms game ownership. A 404 here means the account has no Java
/// Edition profile — i.e. doesn't own the game — and nothing should
/// install or launch.
fn fetch_minecraft_profile(client: &Client, minecraft_access_token: &str) -> Result<MinecraftProfile, MsaError> {
    let response = client
        .get(MINECRAFT_PROFILE_URL)
        .bearer_auth(minecraft_access_token)
        .send()
        .map_err(MsaError::Network)?;
    if response.status().as_u16() == 404 {
        return Err(MsaError::DoesNotOwnMinecraft);
    }
    if !response.status().is_success() {
        return Err(MsaError::HttpStatus(response.status()));
    }
    response.json().map_err(MsaError::Network)
}

pub struct LoginResult {
    pub minecraft_access_token: String,
    pub profile: MinecraftProfile,
    pub refresh_token: String,
    /// Xbox User ID for the `${auth_xuid}` launch argument. Not every
    /// account state returns one; the game works fine with an empty value.
    pub xuid: Option<String>,
}

fn complete_login(client: &Client, tokens: MicrosoftTokens) -> Result<LoginResult, MsaError> {
    let (xbox_live_token, _uhs) = xbox_live_user_token(client, &tokens.access_token)?;
    let (xsts_token, user_hash, xuid) = xsts_authorize(client, &xbox_live_token)?;
    let minecraft_access_token = minecraft_login(client, &user_hash, &xsts_token)?;
    let profile = fetch_minecraft_profile(client, &minecraft_access_token)?;
    Ok(LoginResult { minecraft_access_token, profile, refresh_token: tokens.refresh_token, xuid })
}

pub fn login_with_device_code(client: &Client, start: &DeviceCodeStart) -> Result<LoginResult, MsaError> {
    let tokens = poll_device_code(client, start)?;
    complete_login(client, tokens)
}

pub fn login_with_refresh_token(client: &Client, refresh_token: &str) -> Result<LoginResult, MsaError> {
    let tokens = refresh_microsoft_tokens(client, refresh_token)?;
    complete_login(client, tokens)
}

// ---------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct StoredAccount {
    refresh_token: String,
    saved_at_unix: u64,
}

pub fn save_refresh_token(data_dir: &Path, refresh_token: &str) -> io::Result<()> {
    fs::create_dir_all(data_dir)?;
    let saved_at_unix = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let contents = serde_json::to_vec_pretty(&StoredAccount { refresh_token: refresh_token.to_string(), saved_at_unix }).expect("StoredAccount is serializable");

    let target = data_dir.join(ACCOUNT_FILE);
    let temporary = data_dir.join(".account.json.shacraft.part");
    fs::write(&temporary, contents)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(temporary, target)
}

pub fn load_refresh_token(data_dir: &Path) -> Option<String> {
    let contents = fs::read_to_string(data_dir.join(ACCOUNT_FILE)).ok()?;
    let account: StoredAccount = serde_json::from_str(&contents).ok()?;
    Some(account.refresh_token)
}

pub fn clear_account(data_dir: &Path) -> io::Result<()> {
    match fs::remove_file(data_dir.join(ACCOUNT_FILE)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_stored_refresh_token() {
        let dir = std::env::temp_dir().join(format!("shacraft-msa-test-{}", std::process::id()));
        assert!(load_refresh_token(&dir).is_none());
        save_refresh_token(&dir, "super-secret-refresh-token").unwrap();
        assert_eq!(load_refresh_token(&dir).as_deref(), Some("super-secret-refresh-token"));
        clear_account(&dir).unwrap();
        assert!(load_refresh_token(&dir).is_none());
        fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn stored_account_file_is_not_world_or_group_readable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("shacraft-msa-perm-test-{}", std::process::id()));
        save_refresh_token(&dir, "secret").unwrap();
        let mode = fs::metadata(dir.join(ACCOUNT_FILE)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn refuses_to_run_with_placeholder_client_id() {
        assert!(!is_configured());
        let client = Client::builder().build().unwrap();
        assert!(matches!(start_device_code(&client), Err(MsaError::NotConfigured)));
    }

    /// Live smoke test: requests a real device code from Microsoft and
    /// checks the shape of the response. Does not (and cannot, without a
    /// human) complete the actual sign-in. Needs `MSA_CLIENT_ID` set to a
    /// real, approved Azure app id first — see the module doc comment.
    /// Run with `cargo test -- --ignored live_requests_device_code`.
    #[test]
    #[ignore]
    fn live_requests_device_code() {
        let client = Client::builder().build().unwrap();
        let start = start_device_code(&client).unwrap();
        assert!(!start.user_code.is_empty());
        assert!(start.verification_uri.starts_with("https://"));
        assert!(start.expires_in_seconds > 0);
    }
}
