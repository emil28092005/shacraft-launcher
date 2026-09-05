use crate::manifest::{self, Manifest};
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use reqwest::{blocking::Client, redirect::Policy};
use serde::Deserialize;
use std::{fmt, time::Duration};

const AERONAUTICS_MANIFEST: &str =
    "https://shacraft.ru/api/launcher/v2/profiles/aeronautics/signed-manifest";
const MANIFEST_PUBLIC_KEY: &str = "2S3FRdZj4Xw5nJpZ3IhqVITBg3nTH9AtGSo1Ew9+qVQ=";

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
    InvalidSignature,
    InvalidManifest(manifest::ManifestError),
}

impl fmt::Display for RemoteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownProfile => formatter.write_str("Unknown ShaCraft profile"),
            Self::Network(error) => write!(formatter, "Cannot load ShaCraft manifest: {error}"),
            Self::Status(status) => write!(formatter, "ShaCraft manifest request failed: {status}"),
            Self::TooLarge => formatter.write_str("ShaCraft manifest is too large"),
            Self::InvalidSignature => formatter.write_str("ShaCraft manifest signature is invalid"),
            Self::InvalidManifest(error) => write!(formatter, "ShaCraft manifest is invalid: {error}"),
        }
    }
}

pub fn fetch_manifest(profile_id: &str) -> Result<Manifest, RemoteError> {
    let url = match profile_id {
        "aeronautics" => AERONAUTICS_MANIFEST,
        _ => return Err(RemoteError::UnknownProfile),
    };
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(Policy::none())
        .build()
        .map_err(RemoteError::Network)?;
    let response = client.get(url).send().map_err(RemoteError::Network)?;
    if !response.status().is_success() {
        return Err(RemoteError::Status(response.status()));
    }
    if response.content_length().is_some_and(|size| size > 2 * 1024 * 1024) {
        return Err(RemoteError::TooLarge);
    }
    let source = response.text().map_err(RemoteError::Network)?;
    let envelope = serde_json::from_str::<SignedManifest>(&source)
        .map_err(|_| RemoteError::InvalidSignature)?;
    if envelope.schema_version != 1 || envelope.key_id != "2026-09-06" {
        return Err(RemoteError::InvalidSignature);
    }
    let payload = STANDARD.decode(envelope.payload).map_err(|_| RemoteError::InvalidSignature)?;
    let signature_bytes = STANDARD.decode(envelope.signature).map_err(|_| RemoteError::InvalidSignature)?;
    let public_key_bytes = STANDARD.decode(MANIFEST_PUBLIC_KEY).expect("embedded public key must be valid");
    let public_key = VerifyingKey::from_bytes(&public_key_bytes.try_into().expect("embedded public key must be 32 bytes"))
        .expect("embedded public key must be valid");
    let signature = Signature::from_slice(&signature_bytes).map_err(|_| RemoteError::InvalidSignature)?;
    public_key.verify(&payload, &signature).map_err(|_| RemoteError::InvalidSignature)?;
    let payload = String::from_utf8(payload).map_err(|_| RemoteError::InvalidSignature)?;
    manifest::validate_json(&payload).map_err(RemoteError::InvalidManifest)
}
