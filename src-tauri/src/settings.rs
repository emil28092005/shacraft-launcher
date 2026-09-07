use crate::download;
use serde::{Deserialize, Serialize};
use std::{fmt, fs, io, path::Path};

const SETTINGS_FILE: &str = "settings.json";
const MIN_MEMORY_MB: u16 = 3 * 1024;
const MAX_MEMORY_MB: u16 = 12 * 1024;
const DEFAULT_MEMORY_MB: u16 = 6 * 1024;
const DEFAULT_NICKNAME: &str = "Emil";

/// Which account the player launches as. `Microsoft` requires a real signed-in
/// session; `Offline` uses the local nickname (no Microsoft account needed).
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AccountMode {
    Microsoft,
    Offline,
}

impl Default for AccountMode {
    fn default() -> Self {
        Self::Offline
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherSettings {
    pub memory_mb: u16,
    #[serde(default = "default_nickname")]
    pub nickname: String,
    #[serde(default)]
    pub account_mode: AccountMode,
}

fn default_nickname() -> String {
    DEFAULT_NICKNAME.into()
}

impl Default for LauncherSettings {
    fn default() -> Self {
        Self {
            memory_mb: DEFAULT_MEMORY_MB,
            nickname: DEFAULT_NICKNAME.into(),
            account_mode: AccountMode::Offline,
        }
    }
}

#[derive(Debug)]
pub enum SettingsError {
    Io(io::Error),
    InvalidJson(serde_json::Error),
    InvalidMemory,
    InvalidNickname,
}

impl fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "Cannot access launcher settings: {error}"),
            Self::InvalidJson(error) => write!(formatter, "Cannot read launcher settings: {error}"),
            Self::InvalidMemory => {
                write!(formatter, "Memory allocation must be between 3 and 12 GiB")
            }
            Self::InvalidNickname => write!(
                formatter,
                "Nickname must be 3-16 ASCII letters, numbers, or underscores"
            ),
        }
    }
}

pub fn load(data_dir: &Path) -> Result<LauncherSettings, SettingsError> {
    let path = data_dir.join(SETTINGS_FILE);
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(LauncherSettings::default())
        }
        Err(error) => return Err(SettingsError::Io(error)),
    };
    let settings = serde_json::from_str(&source).map_err(SettingsError::InvalidJson)?;
    validate(&settings)?;
    Ok(settings)
}

pub fn save(
    data_dir: &Path,
    settings: LauncherSettings,
) -> Result<LauncherSettings, SettingsError> {
    validate(&settings)?;
    fs::create_dir_all(data_dir).map_err(SettingsError::Io)?;

    let target = data_dir.join(SETTINGS_FILE);
    let temporary = data_dir.join(".settings.json.shacraft.part");
    let contents = serde_json::to_vec_pretty(&settings).expect("LauncherSettings is serializable");
    fs::write(&temporary, contents).map_err(SettingsError::Io)?;
    download::replace_file(&temporary, &target).map_err(SettingsError::Io)?;
    Ok(settings)
}

fn validate(settings: &LauncherSettings) -> Result<(), SettingsError> {
    if !(MIN_MEMORY_MB..=MAX_MEMORY_MB).contains(&settings.memory_mb)
        || settings.memory_mb % 1024 != 0
    {
        return Err(SettingsError::InvalidMemory);
    }
    if !(3..=16).contains(&settings.nickname.len())
        || !settings
            .nickname
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(SettingsError::InvalidNickname);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{load, save, AccountMode, LauncherSettings};
    use std::{
        fs, process,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temporary_directory() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "shacraft-settings-test-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn defaults_then_persists_memory() {
        let directory = temporary_directory();
        let default = load(&directory).unwrap();
        assert_eq!(default.memory_mb, 6 * 1024);
        assert_eq!(default.nickname, "Emil");
        assert_eq!(default.account_mode, AccountMode::Offline);

        let saved = save(
            &directory,
            LauncherSettings {
                memory_mb: 8 * 1024,
                nickname: "Emil".into(),
                account_mode: AccountMode::Microsoft,
            },
        )
        .unwrap();
        assert_eq!(saved.memory_mb, 8 * 1024);
        assert_eq!(
            load(&directory).unwrap().account_mode,
            AccountMode::Microsoft
        );

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_unsafe_memory_values() {
        let directory = temporary_directory();
        assert!(save(
            &directory,
            LauncherSettings {
                memory_mb: 512,
                nickname: "Emil".into(),
                account_mode: AccountMode::Offline
            }
        )
        .is_err());
        assert!(save(
            &directory,
            LauncherSettings {
                memory_mb: 6 * 1024,
                nickname: "невалидный".into(),
                account_mode: AccountMode::Offline
            }
        )
        .is_err());
    }

    #[test]
    fn old_settings_without_nickname_defaults_gracefully() {
        let directory = temporary_directory();
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("settings.json"), r#"{"memoryMb": 6144}"#).unwrap();
        let settings = load(&directory).unwrap();
        assert_eq!(settings.memory_mb, 6 * 1024);
        assert_eq!(settings.nickname, "Emil");
        fs::remove_dir_all(directory).unwrap();
    }
}
