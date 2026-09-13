//! Fabric metadata and Maven are fixed, independent game trust domains.
//! The signed ShaCraft manifest selects versions, never arbitrary loader URLs.
use crate::mojang::{Artifact, LibraryDownloads, VersionJson};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::{io::Read, time::Duration};

pub(crate) const MAVEN_HOST: &str = "maven.fabricmc.net";
const HOSTS: [&str; 2] = ["meta.fabricmc.net", MAVEN_HOST];
const MAX_PROFILE: usize = 512 * 1024;

#[derive(Deserialize)]
struct FabricLibrary {
    name: String,
    url: String,
    sha1: Option<String>,
    size: Option<u64>,
}

fn coordinate_path(name: &str) -> Result<String, String> {
    let pieces: Vec<_> = name.split(':').collect();
    if pieces.len() != 3
        || pieces.iter().any(|p| {
            !crate::manifest::is_portable_component(p)
                || *p == "."
                || *p == ".."
                || !p
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._+-".contains(&c))
        })
        || pieces[0].split('.').any(|p| !crate::manifest::is_portable_component(p))
    {
        return Err("Invalid Fabric library coordinate".into());
    }
    Ok(format!(
        "{}/{}/{}/{}-{}.jar",
        pieces[0].replace('.', "/"),
        pieces[1],
        pieces[2],
        pieces[1],
        pieces[2]
    ))
}

fn get(client: &Client, url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let response = client
        .get(url)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    response
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("Fabric metadata exceeds its size limit".into());
    }
    Ok(bytes)
}

pub(crate) fn fetch_profile(minecraft: &str, loader: &str) -> Result<VersionJson, String> {
    let client =
        crate::trusted_http::client(&HOSTS, Duration::from_secs(30)).map_err(|e| e.to_string())?;
    // Defence in depth: these values normally already passed manifest validation.
    if [minecraft, loader].iter().any(|v| {
        !crate::manifest::is_portable_component(v)
            || v.contains('/')
            || !v
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._+-".contains(&c))
    }) {
        return Err("Invalid Fabric version".into());
    }
    let bytes = get(
        &client,
        &format!("https://meta.fabricmc.net/v2/versions/loader/{minecraft}/{loader}/profile/json"),
        MAX_PROFILE,
    )?;
    normalize(&client, &bytes, minecraft, loader)
}

fn normalize(
    client: &Client,
    bytes: &[u8],
    minecraft: &str,
    loader: &str,
) -> Result<VersionJson, String> {
    // Flattening libraries would consume the same key twice; parse the two views explicitly.
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let parent = value.get("inheritsFrom").and_then(|v| v.as_str());
    let mut version: VersionJson =
        serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    if parent != Some(minecraft)
        || version.id != format!("fabric-loader-{loader}-{minecraft}")
        || version.main_class != "net.fabricmc.loader.impl.launch.knot.KnotClient"
    {
        return Err("Fabric profile identity mismatch".into());
    }
    let artifacts: Vec<FabricLibrary> =
        serde_json::from_value(value["libraries"].clone()).map_err(|e| e.to_string())?;
    if artifacts.is_empty() || artifacts.len() > 32 {
        return Err("Invalid Fabric library count".into());
    }
    for (library, artifact) in version.libraries.iter_mut().zip(artifacts) {
        if artifact.url != "https://maven.fabricmc.net/" {
            return Err("Untrusted Fabric Maven URL".into());
        }
        let path = coordinate_path(&artifact.name)?;
        let url = format!("https://{MAVEN_HOST}/{path}");
        let sha1 = match artifact.sha1 {
            Some(hash) => hash,
            None => String::from_utf8(get(client, &format!("{url}.sha1"), 128)?)
                .map_err(|e| e.to_string())?
                .trim()
                .to_owned(),
        };
        let size = match artifact.size {
            Some(size) => size,
            None => client
                .head(&url)
                .send()
                .map_err(|e| e.to_string())?
                .error_for_status()
                .map_err(|e| e.to_string())?
                .headers()
                .get(reqwest::header::CONTENT_LENGTH)
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or("Fabric library has no size")?,
        };
        if sha1.len() != 40
            || !sha1.bytes().all(|b| b.is_ascii_hexdigit())
            || size == 0
            || size > 64 * 1024 * 1024
        {
            return Err("Invalid Fabric library hash or size".into());
        }
        library.downloads = Some(LibraryDownloads {
            artifact: Some(Artifact {
                path,
                url,
                sha1,
                size,
            }),
        });
    }
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maven_coordinates_cannot_escape_library_directory() {
        assert_eq!(
            coordinate_path("net.fabricmc:fabric-loader:0.19.5").unwrap(),
            "net/fabricmc/fabric-loader/0.19.5/fabric-loader-0.19.5.jar"
        );
        for bad in [
            "x:y:../evil",
            "a..b:c:1",
            "x:/tmp:1",
            "x:y:1:extra",
            "x:y:\\evil",
            "x:y:..",
            "CON:y:1",
            "a:y.:1",
            "a:AUX:1",
        ] {
            assert!(coordinate_path(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn loader_identity_and_maven_origin_are_checked_before_downloads() {
        let client = Client::new();
        let base = serde_json::json!({"id":"fabric-loader-0.19.5-26.2", "inheritsFrom":"26.2", "mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient", "libraries":[{"name":"net.fabricmc:fabric-loader:0.19.5", "url":"https://maven.fabricmc.net/", "sha1":"a".repeat(40), "size":42}]});
        assert!(normalize(
            &client,
            &serde_json::to_vec(&base).unwrap(),
            "26.2",
            "0.19.5"
        )
        .is_ok());
        for (field, value) in [
            ("inheritsFrom", "1.21.1"),
            ("mainClass", "attacker.Main"),
            ("id", "wrong"),
        ] {
            let mut bad = base.clone();
            bad[field] = value.into();
            assert!(normalize(
                &client,
                &serde_json::to_vec(&bad).unwrap(),
                "26.2",
                "0.19.5"
            )
            .is_err());
        }
        let mut bad = base;
        bad["libraries"][0]["url"] = "https://attacker.invalid/".into();
        assert!(normalize(
            &client,
            &serde_json::to_vec(&bad).unwrap(),
            "26.2",
            "0.19.5"
        )
        .is_err());
    }
    #[test]
    #[ignore = "downloads official Fabric profile metadata"]
    fn live_resolves_fabric_26_2() {
        let profile = fetch_profile("26.2", "0.19.5").unwrap();
        assert!(profile
            .libraries
            .iter()
            .all(|library| library.downloads.is_some()));
    }
}
