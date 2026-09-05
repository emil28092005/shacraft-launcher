use crate::manifest::{self, Manifest};
use reqwest::{blocking::Client, redirect::Policy};
use std::{fmt, time::Duration};

const AERONAUTICS_MANIFEST: &str =
    "https://shacraft.ru/api/launcher/v2/profiles/aeronautics/manifest";

#[derive(Debug)]
pub enum RemoteError {
    UnknownProfile,
    Network(reqwest::Error),
    Status(reqwest::StatusCode),
    TooLarge,
    InvalidManifest(manifest::ManifestError),
}

impl fmt::Display for RemoteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownProfile => formatter.write_str("Unknown ShaCraft profile"),
            Self::Network(error) => write!(formatter, "Cannot load ShaCraft manifest: {error}"),
            Self::Status(status) => write!(formatter, "ShaCraft manifest request failed: {status}"),
            Self::TooLarge => formatter.write_str("ShaCraft manifest is too large"),
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
    manifest::validate_json(&source).map_err(RemoteError::InvalidManifest)
}
