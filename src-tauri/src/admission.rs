//! Ephemeral proof of possession for one Aeronautics connection.
//!
//! Neither this private key nor its ticket crosses IPC, enters launch arguments,
//! or is persisted. Only the new Java child's environment receives them. A new
//! launch obtains a new key and ticket; account session credentials stay native.

use crate::session::PlayerIdentity;
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use ed25519_dalek::{
    pkcs8::{EncodePrivateKey, KeypairBytes},
    SigningKey,
};
use serde::{Deserialize, Serialize};
use std::process::Command;
use zeroize::Zeroizing;

pub(crate) const TICKET_ENV: &str = "SHACRAFT_ADMISSION_TICKET";
pub(crate) const PRIVATE_KEY_ENV: &str = "SHACRAFT_ADMISSION_PRIVATE_KEY";
const SERVER_ID: &str = "aoc";
const MAX_LIFETIME_SECONDS: u64 = 600;

// Deliberately no Debug, Clone or Serialize for secret-bearing values.
pub(crate) struct AdmissionKey {
    public_key: String,
    private_key: Zeroizing<String>,
}

#[derive(Serialize)]
pub(crate) struct TicketRequest<'a> {
    server_id: &'static str,
    public_key: &'a str,
}

#[derive(Deserialize)]
pub(crate) struct TicketResponse {
    ticket_id: String,
    mc_username: String,
    server_id: String,
    expires_in_seconds: u64,
}

pub(crate) struct Admission {
    ticket_id: Zeroizing<String>,
    private_key: Zeroizing<String>,
    identity: PlayerIdentity,
}

impl AdmissionKey {
    pub(crate) fn generate() -> Result<Self, &'static str> {
        let mut seed = Zeroizing::new([0_u8; 32]);
        getrandom::fill(seed.as_mut())
            .map_err(|_| "Не удалось создать защищённый ключ входа. Повторите запуск лаунчера.")?;
        let signing_key = SigningKey::from_bytes(&seed);
        let public_key = STANDARD.encode(signing_key.verifying_key().to_bytes());
        // RFC 8410 PKCS#8 v1 (PrivateKeyInfo) without the optional public key.
        // This is accepted by Java 21's Ed25519 KeyFactory/PKCS8EncodedKeySpec.
        let key_bytes = KeypairBytes {
            secret_key: signing_key.to_bytes(),
            public_key: None,
        };
        let encoded = key_bytes
            .to_pkcs8_der()
            .map_err(|_| "Не удалось подготовить защищённый ключ входа.")?;
        Ok(Self {
            public_key,
            private_key: Zeroizing::new(STANDARD.encode(encoded.as_bytes())),
        })
    }

    pub(crate) fn request(&self) -> TicketRequest<'_> {
        TicketRequest {
            server_id: SERVER_ID,
            public_key: &self.public_key,
        }
    }

    pub(crate) fn bind(self, response: TicketResponse) -> Result<Admission, &'static str> {
        let ticket_id = Zeroizing::new(response.ticket_id);
        let valid_ticket = ticket_id.len() == 43
            && URL_SAFE_NO_PAD
                .decode(ticket_id.as_bytes())
                .is_ok_and(|bytes| {
                    bytes.len() == 32 && URL_SAFE_NO_PAD.encode(bytes) == *ticket_id
                });
        let valid_nickname = (3..=16).contains(&response.mc_username.len())
            && response
                .mc_username
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
        if !valid_ticket
            || !valid_nickname
            || response.server_id != SERVER_ID
            || !(1..=MAX_LIFETIME_SECONDS).contains(&response.expires_in_seconds)
        {
            return Err("Сервер вернул некорректное разрешение на вход. Повторите попытку позже.");
        }
        Ok(Admission {
            ticket_id,
            private_key: self.private_key,
            identity: PlayerIdentity::Offline {
                name: response.mc_username,
            },
        })
    }
}

impl Admission {
    pub(crate) fn identity(&self) -> &PlayerIdentity {
        &self.identity
    }

    pub(crate) fn configure_child(&self, command: &mut Command) {
        command.env(TICKET_ENV, self.ticket_id.as_str());
        command.env(PRIVATE_KEY_ENV, self.private_key.as_str());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{pkcs8::DecodePrivateKey, Signer, Verifier};

    fn response() -> TicketResponse {
        TicketResponse {
            ticket_id: URL_SAFE_NO_PAD.encode([37_u8; 32]),
            mc_username: "Canonical_Name".into(),
            server_id: SERVER_ID.into(),
            expires_in_seconds: 600,
        }
    }

    #[test]
    fn generates_distinct_keys_and_only_sends_the_public_key() {
        let key = AdmissionKey::generate().unwrap();
        let other = AdmissionKey::generate().unwrap();
        assert_ne!(key.public_key, other.public_key);
        let payload = serde_json::to_value(key.request()).unwrap();
        assert_eq!(payload.as_object().unwrap().len(), 2);
        assert_eq!(payload["server_id"], SERVER_ID);
        assert_eq!(payload["public_key"], key.public_key);
        assert!(!payload.to_string().contains(key.private_key.as_str()));
        let der = Zeroizing::new(STANDARD.decode(key.private_key.as_bytes()).unwrap());
        let restored = SigningKey::from_pkcs8_der(&der).unwrap();
        assert_eq!(
            STANDARD.encode(restored.verifying_key().to_bytes()),
            key.public_key
        );
        let message = b"shacraft-admission-v1:challenge-fixture";
        restored
            .verifying_key()
            .verify(message, &restored.sign(message))
            .unwrap();
        // Java's standard Ed25519 encoding is the 48-byte private-key-only form.
        assert_eq!(der.len(), 48);
    }

    #[test]
    fn rejects_untrusted_identity_ticket_server_and_expiry() {
        let mutations: Vec<Box<dyn Fn(&mut TicketResponse)>> = vec![
            Box::new(|r| r.ticket_id = "../unsafe".into()),
            Box::new(|r| r.ticket_id = "A".repeat(42) + "!"),
            Box::new(|r| r.ticket_id = "A".repeat(42) + "B"),
            Box::new(|r| r.mc_username = "../../outside".into()),
            Box::new(|r| r.mc_username = "ab".into()),
            Box::new(|r| r.mc_username = "a".repeat(17)),
            Box::new(|r| r.server_id = "other".into()),
            Box::new(|r| r.expires_in_seconds = 0),
            Box::new(|r| r.expires_in_seconds = 601),
        ];
        for mutate in mutations {
            let mut payload = response();
            mutate(&mut payload);
            assert!(AdmissionKey::generate().unwrap().bind(payload).is_err());
        }
    }

    #[test]
    fn secrets_only_enter_the_child_environment_and_identity_comes_from_ticket() {
        let original_ticket = std::env::var_os(TICKET_ENV);
        let original_key = std::env::var_os(PRIVATE_KEY_ENV);
        let admission = AdmissionKey::generate().unwrap().bind(response()).unwrap();
        let mut command = Command::new("java");
        command
            .arg("-Xmx6144M")
            .arg("net.minecraft.client.main.Main");
        admission.configure_child(&mut command);
        assert_eq!(admission.identity().name(), "Canonical_Name");
        let env: std::collections::HashMap<_, _> = command.get_envs().collect();
        assert_eq!(
            env.get(std::ffi::OsStr::new(TICKET_ENV)).unwrap().unwrap(),
            admission.ticket_id.as_str()
        );
        assert_eq!(
            env.get(std::ffi::OsStr::new(PRIVATE_KEY_ENV))
                .unwrap()
                .unwrap(),
            admission.private_key.as_str()
        );
        assert_eq!(env.len(), 2);
        for argument in command.get_args() {
            assert!(!argument
                .to_string_lossy()
                .contains(admission.ticket_id.as_str()));
            assert!(!argument
                .to_string_lossy()
                .contains(admission.private_key.as_str()));
        }
        assert_eq!(std::env::var_os(TICKET_ENV), original_ticket);
        assert_eq!(std::env::var_os(PRIVATE_KEY_ENV), original_key);
    }
}
