//! Player identity for launching the game.
//!
//! A profile can launch either as a real Microsoft-authenticated player
//! (see `msa.rs`) or as an offline account. Offline mode is an explicit,
//! opt-in choice in the UI, never a fallback that silently weakens the
//! Microsoft path: if a player has signed in with Microsoft, that session
//! is always preferred when present.
//!
//! Offline identity uses the same algorithm every vanilla offline-mode
//! server relies on (and that the official launcher uses for demo accounts):
//! an MD5 of `OfflinePlayer:<nickname>` with the version-3 (name-based)
//! and RFC 4122 variant bits set, formatted as a dashed UUID. The game
//! derives the same UUID client-side, so `--uuid`/`--accessToken` can be
//! a deterministic placeholder; the server's own `loginsystem` mod
//! (ShaCraft's chosen approach) then authenticates by nickname/password.

use crate::msa;
use md5::{Digest, Md5};

/// A resolved identity used to fill `${auth_player_name}`, `${auth_uuid}`,
/// `${auth_access_token}`, `${user_type}` and friends at launch time.
pub enum PlayerIdentity {
    Microsoft(msa::LoginResult),
    Offline { name: String },
}

impl PlayerIdentity {
    pub fn name(&self) -> &str {
        match self {
            Self::Microsoft(result) => &result.profile.name,
            Self::Offline { name } => name,
        }
    }

    /// The `--accessToken` value. A real Microsoft session passes the
    /// live token; offline mode uses a fixed placeholder because the game
    /// client only requires a non-empty value and the server is in offline
    /// mode with its own auth mod.
    pub fn access_token(&self) -> &str {
        match self {
            Self::Microsoft(result) => &result.minecraft_access_token,
            Self::Offline { .. } => "0",
        }
    }

    /// The dashed UUID for `${auth_uuid}`. Microsoft profiles come back
    /// from Mojang as 32 hex chars; offline mode computes the deterministic
    /// name-based UUID.
    pub fn uuid(&self) -> String {
        match self {
            Self::Microsoft(result) => format_uuid_with_dashes(&result.profile.id),
            Self::Offline { name } => offline_uuid(name),
        }
    }

    /// The `${user_type}` argument: `msa` for Microsoft accounts,
    /// `legacy` for offline mode (the value the vanilla launcher uses for
    /// demo/offline sessions).
    pub fn user_type(&self) -> &'static str {
        match self {
            Self::Microsoft(_) => "msa",
            Self::Offline { .. } => "legacy",
        }
    }

    /// The `${auth_xuid}` argument. Offline mode has no Xbox identity and
    /// passes an empty string, which the game accepts.
    pub fn xuid(&self) -> &str {
        match self {
            Self::Microsoft(result) => result.xuid.as_deref().unwrap_or(""),
            Self::Offline { .. } => "",
        }
    }
}

/// Mojang profile ids come back as 32 hex chars with no dashes; the game
/// itself expects the standard dashed UUID form for `${auth_uuid}`.
pub fn format_uuid_with_dashes(id: &str) -> String {
    if id.len() != 32 || id.contains('-') {
        return id.to_string();
    }
    format!("{}-{}-{}-{}-{}", &id[0..8], &id[8..12], &id[12..16], &id[16..20], &id[20..32])
}

/// Computes the deterministic offline UUID for a nickname, using the same
/// name-based (version 3) algorithm the vanilla server uses for offline
/// players: MD5 of `OfflinePlayer:<nick>` with the version and variant bits
/// set. This is what lets the client and an offline-mode server agree on a
/// player's UUID without any account lookup.
pub fn offline_uuid(nickname: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(format!("OfflinePlayer:{nickname}"));
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest);
    bytes[6] = (bytes[6] & 0x0f) | 0x30; // version 3 (name-based)
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // RFC 4122 variant
    let hex = bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    format_uuid_with_dashes(&hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference values computed from the vanilla server's offline UUID
    /// algorithm; these are stable and must not change.
    #[test]
    fn computes_reference_offline_uuids() {
        assert_eq!(offline_uuid("Emil"), "947b017f-e6de-3cc4-894d-019938ca63d4");
        assert_eq!(offline_uuid("Emil_Shanaty"), "c89467f6-2526-381f-a5c4-c82b677e4150");
        assert_eq!(offline_uuid("Notch"), "b50ad385-829d-3141-a216-7e7d7539ba7f");
        assert_eq!(offline_uuid("Steve"), "5627dd98-e6be-3c21-b8a8-e92344183641");
    }

    #[test]
    fn offline_uuid_has_correct_version_and_variant() {
        let id = offline_uuid("Emil");
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[2].chars().next().unwrap(), '3'); // name-based
        assert!(matches!(parts[3].chars().next().unwrap(), '8' | '9' | 'a' | 'b'));
    }

    #[test]
    fn offline_identity_uses_legacy_user_type() {
        let identity = PlayerIdentity::Offline { name: "Emil".into() };
        assert_eq!(identity.name(), "Emil");
        assert_eq!(identity.user_type(), "legacy");
        assert_eq!(identity.access_token(), "0");
        assert_eq!(identity.uuid(), offline_uuid("Emil"));
    }

    #[test]
    fn formats_dashless_uuid() {
        assert_eq!(
            format_uuid_with_dashes("0123456789abcdef0123456789abcdef"),
            "01234567-89ab-cdef-0123-456789abcdef"
        );
        let dashed = "01234567-89ab-cdef-0123-456789abcdef";
        assert_eq!(format_uuid_with_dashes(dashed), dashed);
    }
}
