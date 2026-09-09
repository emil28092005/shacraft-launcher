use crate::manifest::{self, Manifest};
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, VerifyingKey};
use reqwest::header::ACCEPT_ENCODING;
use reqwest::{blocking::Client, redirect::Policy};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    io::{self, Read},
    thread,
    time::Duration,
};

const AERONAUTICS_MANIFEST: &str =
    "https://shacraft.ru/api/launcher/v2/profiles/aeronautics/signed-manifest";
const AERONAUTICS_ONLINE: &str = "https://shacraft.ru/api/online/aoc";
const MANIFEST_PUBLIC_KEY: &str = "2S3FRdZj4Xw5nJpZ3IhqVITBg3nTH9AtGSo1Ew9+qVQ=";
const MANIFEST_KEY_ID: &str = "2026-09-06";
const MAX_ENVELOPE_BYTES: usize = 2 * 1024 * 1024;
const MANIFEST_ATTEMPTS: u32 = 3;

/// Display-only status: never used to select executable files or versions.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub online: Option<u32>,
    pub max: Option<u32>,
    pub reachable: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignedManifest {
    schema_version: u32,
    key_id: String,
    payload: String,
    signature: String,
}

#[derive(Debug)]
pub enum RemoteError {
    UnknownProfile,
    Network(reqwest::Error),
    Status(reqwest::StatusCode),
    TooLarge,
    Read(io::Error),
    InvalidSignature,
    ProfileMismatch,
    InvalidManifest(manifest::ManifestError),
}

impl fmt::Display for RemoteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownProfile => formatter.write_str("Unknown ShaCraft profile"),
            Self::Network(error) => write!(formatter, "Cannot load ShaCraft manifest: {error}"),
            Self::Status(status) => write!(formatter, "ShaCraft manifest request failed: {status}"),
            Self::TooLarge => formatter.write_str("ShaCraft manifest is too large"),
            Self::Read(error) => write!(formatter, "Cannot read ShaCraft manifest: {error}"),
            Self::InvalidSignature => formatter.write_str("ShaCraft manifest signature is invalid"),
            Self::ProfileMismatch => {
                formatter.write_str("Signed manifest does not match the requested profile")
            }
            Self::InvalidManifest(error) => {
                write!(formatter, "ShaCraft manifest is invalid: {error}")
            }
        }
    }
}

pub fn fetch_manifest(profile_id: &str) -> Result<Manifest, RemoteError> {
    let url = match profile_id {
        "aeronautics" => AERONAUTICS_MANIFEST,
        _ => return Err(RemoteError::UnknownProfile),
    };
    let client = Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .redirect(Policy::none())
        .build()
        .map_err(RemoteError::Network)?;
    let source = fetch_manifest_bytes(&client, url)?;
    let public_key_bytes = STANDARD
        .decode(MANIFEST_PUBLIC_KEY)
        .expect("embedded public key must be valid");
    let public_key = VerifyingKey::from_bytes(
        &public_key_bytes
            .try_into()
            .expect("embedded public key must be 32 bytes"),
    )
    .expect("embedded public key must be valid");
    verify_envelope(&source, profile_id, &public_key)
}

fn fetch_manifest_bytes(client: &Client, url: &str) -> Result<Vec<u8>, RemoteError> {
    for attempt in 1..=MANIFEST_ATTEMPTS {
        let request = || {
            let response = client
                .get(url)
                .header(ACCEPT_ENCODING, "identity")
                .send()
                .map_err(RemoteError::Network)?;
            if !response.status().is_success() {
                return Err(RemoteError::Status(response.status()));
            }
            if response
                .content_length()
                .is_some_and(|size| size > MAX_ENVELOPE_BYTES as u64)
            {
                return Err(RemoteError::TooLarge);
            }
            read_envelope(response)
        };
        match request() {
            Err(RemoteError::Network(_) | RemoteError::Read(_)) if attempt < MANIFEST_ATTEMPTS => {
                thread::sleep(Duration::from_millis(250 * attempt as u64));
            }
            result => return result,
        }
    }
    unreachable!("the last attempt always returns")
}

pub fn fetch_server_status(profile_id: &str) -> Result<ServerStatus, RemoteError> {
    let url = match profile_id {
        "aeronautics" => AERONAUTICS_ONLINE,
        _ => return Err(RemoteError::UnknownProfile),
    };
    let client = Client::builder()
        .https_only(true)
        .timeout(Duration::from_secs(10))
        .redirect(Policy::none())
        .build()
        .map_err(RemoteError::Network)?;
    let response = client.get(url).send().map_err(RemoteError::Network)?;
    if !response.status().is_success() {
        return Err(RemoteError::Status(response.status()));
    }
    response
        .json::<ServerStatus>()
        .map_err(RemoteError::Network)
}

fn read_envelope(source: impl Read) -> Result<Vec<u8>, RemoteError> {
    let mut bytes = Vec::new();
    source
        .take(MAX_ENVELOPE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(RemoteError::Read)?;
    if bytes.len() > MAX_ENVELOPE_BYTES {
        return Err(RemoteError::TooLarge);
    }
    Ok(bytes)
}

fn verify_envelope(
    source: &[u8],
    profile_id: &str,
    public_key: &VerifyingKey,
) -> Result<Manifest, RemoteError> {
    if source.len() > MAX_ENVELOPE_BYTES {
        return Err(RemoteError::TooLarge);
    }
    let envelope = serde_json::from_slice::<SignedManifest>(source)
        .map_err(|_| RemoteError::InvalidSignature)?;
    if envelope.schema_version != 1 || envelope.key_id != MANIFEST_KEY_ID {
        return Err(RemoteError::InvalidSignature);
    }
    let payload = STANDARD
        .decode(envelope.payload)
        .map_err(|_| RemoteError::InvalidSignature)?;
    let signature_bytes = STANDARD
        .decode(envelope.signature)
        .map_err(|_| RemoteError::InvalidSignature)?;
    let signature =
        Signature::from_slice(&signature_bytes).map_err(|_| RemoteError::InvalidSignature)?;
    public_key
        .verify_strict(&payload, &signature)
        .map_err(|_| RemoteError::InvalidSignature)?;
    let payload = String::from_utf8(payload).map_err(|_| RemoteError::InvalidSignature)?;
    let manifest = manifest::validate_json(&payload).map_err(RemoteError::InvalidManifest)?;
    if manifest.id != profile_id {
        return Err(RemoteError::ProfileMismatch);
    }
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::{json, Value};

    #[test]
    #[ignore = "read-only check of the production signed manifest; requires network"]
    fn live_validates_production_aeronautics_manifest() {
        let manifest = fetch_manifest("aeronautics").unwrap();
        assert_eq!(manifest.id, "aeronautics");
        assert!(!manifest.files.is_empty());
    }

    fn signed_fixture() -> (Value, VerifyingKey) {
        let key = SigningKey::from_bytes(&[17; 32]);
        let payload = serde_json::to_vec(&json!({
            "schemaVersion": 1, "id": "aeronautics", "displayName": "Aeronautics",
            "minecraft": {"version": "1.21.1", "loader": {"kind": "neoforge", "version": "21.1.248"}, "javaMajor": 21},
            "files": []
        })).unwrap();
        let envelope = json!({
            "schemaVersion": 1, "keyId": MANIFEST_KEY_ID,
            "payload": STANDARD.encode(&payload),
            "signature": STANDARD.encode(key.sign(&payload).to_bytes())
        });
        (envelope, key.verifying_key())
    }

    #[test]
    fn accepts_valid_signature_and_binds_requested_profile() {
        let (envelope, key) = signed_fixture();
        let bytes = serde_json::to_vec(&envelope).unwrap();
        assert_eq!(
            verify_envelope(&bytes, "aeronautics", &key).unwrap().id,
            "aeronautics"
        );
        assert!(matches!(
            verify_envelope(&bytes, "another-profile", &key),
            Err(RemoteError::ProfileMismatch)
        ));
    }

    #[test]
    fn rejects_modified_payload_signature_key_and_schema() {
        let (original, key) = signed_fixture();
        for (field, value) in [
            ("payload", json!(STANDARD.encode(b"{}"))),
            ("signature", json!(STANDARD.encode([0; 64]))),
            ("keyId", json!("unknown")),
            ("schemaVersion", json!(2)),
        ] {
            let mut envelope = original.clone();
            envelope[field] = value;
            assert!(
                matches!(
                    verify_envelope(&serde_json::to_vec(&envelope).unwrap(), "aeronautics", &key),
                    Err(RemoteError::InvalidSignature)
                ),
                "{field}"
            );
        }
        let other_key = SigningKey::from_bytes(&[18; 32]).verifying_key();
        assert!(matches!(
            verify_envelope(
                &serde_json::to_vec(&original).unwrap(),
                "aeronautics",
                &other_key
            ),
            Err(RemoteError::InvalidSignature)
        ));
    }

    #[test]
    fn bounds_stream_without_content_length() {
        assert!(matches!(
            read_envelope(io::repeat(b'x')),
            Err(RemoteError::TooLarge)
        ));
    }

    #[test]
    fn retries_truncated_manifest_transfers_and_requests_identity_encoding() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/manifest", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for body in [
                b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nbad".as_slice(),
                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".as_slice(),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = [0_u8; 4096];
                let length = stream.read(&mut request).unwrap();
                assert!(String::from_utf8_lossy(&request[..length])
                    .to_ascii_lowercase()
                    .contains("accept-encoding: identity"));
                stream.write_all(body).unwrap();
            }
        });
        let client = Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        assert_eq!(fetch_manifest_bytes(&client, &url).unwrap(), b"{}");
        server.join().unwrap();
    }
}
