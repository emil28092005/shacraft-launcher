use super::*;
use std::{
    cell::Cell,
    collections::VecDeque,
    io::{self, Cursor},
};

const KEY: &str = include_str!("../../../tests/fixtures/updater/public-key.txt");
const METADATA: &[u8] = include_bytes!("../../../tests/fixtures/updater/latest.json");
const SIG: &str = include_str!("../../../tests/fixtures/updater/latest.json.sig");
const PACKAGE: &[u8] = include_bytes!("../../../tests/fixtures/updater/package.txt");

fn release() -> VerifiedRelease {
    VerifiedRelease::parse(METADATA, SIG, KEY).unwrap()
}

#[test]
fn real_tauri_signatures_authenticate_metadata_and_fake_installer_input() {
    assert!(key_is_valid(KEY));
    assert!(!key_is_valid("unconfigured"));
    let release = release();
    let artifact = &release.release.platforms["linux-x86_64"];
    let calls = Cell::new(0);
    install_verified(PACKAGE, artifact, KEY, |bytes| {
        calls.set(calls.get() + 1);
        assert_eq!(bytes, PACKAGE);
        Ok(())
    })
    .unwrap();
    assert_eq!(calls.get(), 1);
    // Real signed test data only. This fake installer never executes the fixture.
}

#[test]
fn tampered_metadata_version_platform_or_package_cannot_reach_installer() {
    for (from, to) in [
        ("0.3.0", "0.9.0"),
        ("darwin-aarch64", "darwin-mips64"),
        ("shacraft-launcher/releases", "other-repository/releases"),
    ] {
        let changed = String::from_utf8(METADATA.to_vec())
            .unwrap()
            .replace(from, to);
        assert!(VerifiedRelease::parse(changed.as_bytes(), SIG, KEY).is_err());
    }
    let release = release();
    let artifact = &release.release.platforms["linux-x86_64"];
    let calls = Cell::new(0);
    let mut corrupt = PACKAGE.to_vec();
    corrupt[0] ^= 1;
    assert!(install_verified(&corrupt, artifact, KEY, |_| {
        calls.set(1);
        Ok(())
    })
    .is_err());
    let mut wrong_signature = artifact.clone();
    wrong_signature.signature = release.release.platforms["windows-x86_64"]
        .signature
        .clone();
    assert!(install_verified(PACKAGE, &wrong_signature, KEY, |_| {
        calls.set(1);
        Ok(())
    })
    .is_err());
    assert_eq!(calls.get(), 0);
}

#[test]
fn same_version_is_no_update_and_downgrades_or_prereleases_are_rejected() {
    let release = release();
    assert!(release.is_newer_than("0.2.0").unwrap());
    assert!(!release.is_newer_than("0.3.0").unwrap());
    assert!(release.is_newer_than("0.4.0").is_err());
    for bad in [
        "v0.3.0",
        "0.3.0-rc.1",
        "0.3.0+other",
        "01.2.3",
        "256.0.0",
        "0.256.0",
        "0.0.65536",
    ] {
        assert!(stable_version(bad).is_err(), "{bad}");
    }
}

#[test]
fn all_platform_descriptors_bind_repository_tag_filename_size_and_hash() {
    let release = release();
    let mut missing = release.release.platforms.clone();
    missing.remove("darwin-aarch64");
    assert!(validate_artifacts(&missing, &PLATFORMS, &release.release).is_err());
    let mut wrong_arch = release.release.platforms.clone();
    wrong_arch.insert(
        "darwin-aarch64".into(),
        release.release.platforms["darwin-x86_64"].clone(),
    );
    assert!(validate_artifacts(&wrong_arch, &PLATFORMS, &release.release).is_err());
    for url in [
        "https://github.com/other/repo/releases/download/v0.3.0/file",
        "https://github.com/emil28092005/shacraft-launcher/releases/download/v0.2.0/file",
        "https://evil.invalid/update",
    ] {
        let mut wrong = release.release.platforms.clone();
        wrong.get_mut("linux-x86_64").unwrap().url = url.into();
        assert!(validate_artifacts(&wrong, &PLATFORMS, &release.release).is_err());
    }
    for size in [0, MAX_PACKAGE + 1] {
        let mut wrong = release.release.platforms.clone();
        wrong.get_mut("linux-x86_64").unwrap().size = size;
        assert!(validate_artifacts(&wrong, &PLATFORMS, &release.release).is_err());
    }
    let mut wrong = release.release.platforms.clone();
    wrong.get_mut("linux-x86_64").unwrap().sha256 = "A".repeat(64);
    assert!(validate_artifacts(&wrong, &PLATFORMS, &release.release).is_err());
}

#[test]
fn github_redirects_cannot_leave_release_hosts_or_downgrade_https() {
    for url in [
        LATEST,
        "https://github.com/emil28092005/shacraft-launcher/releases/download/v0.3.0/latest.json",
        "https://release-assets.githubusercontent.com/github-production-release-asset/123?sig=test",
    ] {
        assert!(redirect_allowed(&Url::parse(url).unwrap()), "{url}");
    }
    for url in [
        "http://github.com/emil28092005/shacraft-launcher/releases",
        "https://github.com/other/repository/releases/latest",
        "https://release-assets.githubusercontent.com.evil.invalid/file",
        "https://evil.invalid/file",
        "https://user@github.com/emil28092005/shacraft-launcher/releases/a",
        "https://github.com:8443/emil28092005/shacraft-launcher/releases/a",
        "https://github.com/emil28092005/shacraft-launcher/releases/a#extra",
    ] {
        assert!(!redirect_allowed(&Url::parse(url).unwrap()), "{url}");
    }
}

#[test]
fn bounded_reads_reject_oversized_or_interrupted_downloads() {
    assert!(read_bounded(Cursor::new(b"12345"), 4, |_| {}).is_err());
    assert_eq!(
        read_bounded(Cursor::new(b"1234"), 4, |_| {}).unwrap(),
        b"1234"
    );
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("test disconnect"))
        }
    }
    assert!(read_bounded(Broken, 10, |_| {}).is_err());
}

#[test]
fn network_failure_and_bad_signature_retry_fetch_whole_signed_release() {
    let mut replies = VecDeque::from([
        Err("offline".into()),
        Ok(Some(METADATA.to_vec())),
        Ok(Some(b"bad-signature".to_vec())),
        Ok(Some(METADATA.to_vec())),
        Ok(Some(SIG.as_bytes().to_vec())),
    ]);
    let mut urls = Vec::new();
    let mut fetch = |url: &str, _| {
        urls.push(url.to_string());
        replies.pop_front().unwrap()
    };
    assert!(fetch_release(&mut fetch, KEY, "0.2.0").is_err());
    assert!(fetch_release(&mut fetch, KEY, "0.2.0").is_err());
    assert_eq!(
        fetch_release(&mut fetch, KEY, "0.2.0")
            .unwrap()
            .unwrap()
            .release
            .version,
        "0.3.0"
    );
    assert_eq!(
        urls,
        [
            LATEST,
            LATEST,
            &format!("{LATEST}.sig"),
            LATEST,
            &format!("{LATEST}.sig")
        ]
    );
    assert!(fetch_release(|_, _| Ok(None), KEY, "0.2.0")
        .unwrap()
        .is_none());
}

#[test]
fn installer_failure_is_reported_and_a_fresh_attempt_reverifies_input() {
    let release = release();
    let artifact = &release.release.platforms["linux-x86_64"];
    assert!(install_verified(PACKAGE, artifact, KEY, |_| Err::<(), _>(
        "simulated installer failure".into()
    ))
    .is_err());
    assert!(install_verified(PACKAGE, artifact, KEY, |_| Ok(())).is_ok());
}
