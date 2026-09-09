use crate::download::{self, Checksum, DownloadError};
use crate::inventory::{self, Change, Fingerprint, Inventory, OwnedFile, Store};
use crate::manifest::{is_allowed_download_url, FilePolicy, ManagedFile, Manifest};
use reqwest::{blocking::Client, redirect::Policy};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashSet},
    fmt, fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInspection {
    pub root: String,
    pub managed_files: usize,
    pub missing_files: usize,
    pub mismatched_files: usize,
    pub stale_files: usize,
    pub conflicts: Vec<String>,
    pub pending_update: bool,
    pub legacy_files: usize,
    pub up_to_date: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub root: String,
    pub downloaded_files: usize,
    pub reused_files: usize,
    pub downloaded_bytes: u64,
    pub removed_files: usize,
}

#[derive(Debug)]
pub enum ProfileError {
    Io(io::Error),
    Network(reqwest::Error),
    Download { path: String, source: DownloadError },
    UnsafePath(PathBuf),
    Conflict(Vec<String>),
}
impl fmt::Display for ProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "Cannot access profile: {error}"),
            Self::Network(error) => write!(formatter, "Cannot download profile file: {error}"),
            Self::Download { path, source } => write!(formatter, "Download failed for {path}: {source}"),
            Self::UnsafePath(path) => write!(formatter, "Unsafe or protected profile path: {}", path.display()),
            Self::Conflict(paths) => write!(formatter, "Файлы изменены или не принадлежат лаунчеру; сохранены без изменений: {}. Проверьте список пользовательских модов.", paths.join(", ")),
        }
    }
}

fn expected_fingerprint(file: &ManagedFile) -> Fingerprint {
    Fingerprint {
        size: file.size,
        sha256: file.sha256.to_ascii_lowercase(),
    }
}
#[cfg(test)]
fn manifest_snapshot(manifest: &Manifest) -> String {
    // Internal identity of the already verified manifest's installation inputs.
    // This fingerprint never substitutes for remote signature verification.
    let mut hash = Sha256::new();
    for value in [
        &manifest.id,
        &manifest.minecraft.version,
        &manifest.minecraft.loader.kind,
        &manifest.minecraft.loader.version,
    ] {
        hash.update(value.as_bytes());
        hash.update([0]);
    }
    hash.update([manifest.minecraft.java_major]);
    for file in &manifest.files {
        for value in [&file.path, &file.url, &file.sha256] {
            hash.update(value.as_bytes());
            hash.update([0]);
        }
        hash.update(file.size.to_le_bytes());
        hash.update([if file.policy == FilePolicy::Managed {
            1
        } else {
            2
        }]);
    }
    format!("{:x}", hash.finalize())
}
fn current(root: &Path, relative: &str) -> Result<Option<Fingerprint>, ProfileError> {
    inventory::fingerprint(&managed_target(root, relative)?).map_err(ProfileError::Io)
}

pub fn inspect(root: &Path, manifest: &Manifest) -> Result<ProfileInspection, ProfileError> {
    let store = Store::open(root).map_err(ProfileError::Io)?;
    let owned = store.load().map_err(ProfileError::Io)?;
    let pending_update = store.pending().map_err(ProfileError::Io)?;
    let mut missing_files = 0;
    let mut mismatched_files = 0;
    let mut conflicts = Vec::new();
    let paths: HashSet<_> = manifest
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    for expected in &manifest.files {
        let actual = current(root, &expected.path)?;
        if actual.is_none() {
            missing_files += 1;
            continue;
        }
        if expected.policy == FilePolicy::Seed {
            continue;
        }
        if actual.as_ref() != Some(&expected_fingerprint(expected)) {
            mismatched_files += 1;
            if owned.files.get(&expected.path).map(|f| &f.fingerprint) != actual.as_ref() {
                conflicts.push(expected.path.clone());
            }
        }
    }
    let mut stale_files = 0;
    for (path, previous) in &owned.files {
        if paths.contains(path.as_str()) {
            continue;
        }
        if let Some(actual) = current(root, path)? {
            stale_files += 1;
            if actual != previous.fingerprint {
                conflicts.push(path.clone());
            }
        }
    }
    let legacy_files = legacy_mods(root, manifest, &owned)?.len();
    Ok(ProfileInspection {
        root: root.display().to_string(),
        managed_files: manifest.files.len(),
        missing_files,
        mismatched_files,
        stale_files,
        pending_update,
        legacy_files,
        up_to_date: missing_files == 0
            && mismatched_files == 0
            && stale_files == 0
            && conflicts.is_empty()
            && !pending_update,
        conflicts,
    })
}

pub fn sync_snapshot(
    root: &Path,
    snapshot: &crate::remote::VerifiedSnapshot,
) -> Result<SyncResult, ProfileError> {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(10 * 60))
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 {
                attempt.error("too many redirects")
            } else if is_allowed_download_url(attempt.url().as_str()) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(ProfileError::Network)?;
    sync_with_snapshot(root, &snapshot.manifest, &snapshot.digest, |file, stage| {
        download_managed_file(&client, file, stage)
    })
}

#[cfg(test)]
fn sync_with(
    root: &Path,
    manifest: &Manifest,
    download: impl FnMut(&ManagedFile, &Path) -> Result<u64, ProfileError>,
) -> Result<SyncResult, ProfileError> {
    sync_with_snapshot(root, manifest, &manifest_snapshot(manifest), download)
}

fn sync_with_snapshot(
    root: &Path,
    manifest: &Manifest,
    snapshot: &str,
    mut download: impl FnMut(&ManagedFile, &Path) -> Result<u64, ProfileError>,
) -> Result<SyncResult, ProfileError> {
    let store = Store::open(root).map_err(ProfileError::Io)?;
    store.recover().map_err(ProfileError::Io)?;
    let previous = store.load().map_err(ProfileError::Io)?;
    let mut next = previous.clone();
    let mut conflicts = Vec::new();
    let mut changes = Vec::new();
    let mut downloads: Vec<(usize, &ManagedFile)> = Vec::new();
    let mut reused_files = 0;
    let mut removed_files = 0;
    let paths: HashSet<_> = manifest
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    for expected in &manifest.files {
        let actual = current(root, &expected.path)?;
        let fingerprint = expected_fingerprint(expected);
        if expected.policy == FilePolicy::Seed && actual.is_some() {
            next.files.remove(&expected.path); // relinquish managed -> seed, preserving edits
            reused_files += 1;
            continue;
        }
        if actual.as_ref() == Some(&fingerprint) {
            // Matching legacy bytes prove content, never launcher ownership.
            if previous.files.get(&expected.path).map(|f| &f.fingerprint) != actual.as_ref() {
                next.files.remove(&expected.path);
            }
            reused_files += 1;
            continue;
        }
        if actual.is_some()
            && previous.files.get(&expected.path).map(|f| &f.fingerprint) != actual.as_ref()
        {
            conflicts.push(expected.path.clone());
            continue;
        }
        downloads.push((changes.len(), expected));
        changes.push(Change {
            path: expected.path.clone(),
            before: actual,
            after: Some(fingerprint.clone()),
        });
        if expected.policy == FilePolicy::Managed {
            next.files.insert(
                expected.path.clone(),
                OwnedFile {
                    fingerprint,
                    snapshot: snapshot.to_owned(),
                },
            );
        } else {
            next.files.remove(&expected.path);
        }
    }
    for (path, previous_file) in &previous.files {
        if paths.contains(path.as_str()) {
            continue;
        }
        match current(root, path)? {
            None => {
                next.files.remove(path);
            }
            Some(actual) if actual == previous_file.fingerprint => {
                changes.push(Change {
                    path: path.clone(),
                    before: Some(actual),
                    after: None,
                });
                next.files.remove(path);
                removed_files += 1;
            }
            Some(_) => conflicts.push(path.clone()),
        }
    }
    if !conflicts.is_empty() {
        return Err(ProfileError::Conflict(conflicts));
    }
    if changes.is_empty() && next == previous {
        return Ok(SyncResult {
            root: root.display().to_string(),
            downloaded_files: 0,
            reused_files,
            downloaded_bytes: 0,
            removed_files: 0,
        });
    }
    let transaction = store.transaction().map_err(ProfileError::Io)?;
    let mut downloaded_bytes = 0;
    for (index, file) in &downloads {
        let stage = store
            .stage(&transaction, *index)
            .map_err(ProfileError::Io)?;
        downloaded_bytes += download(file, &stage)?;
        if inventory::fingerprint(&stage)
            .map_err(ProfileError::Io)?
            .as_ref()
            != Some(&expected_fingerprint(file))
        {
            return Err(ProfileError::Io(inventory::invalid(
                "Staged profile file failed verification",
            )));
        }
    }
    // Persist the whole future ownership set before the first payload change.
    store
        .prepare(transaction, changes, next)
        .map_err(ProfileError::Io)?;
    store.recover().map_err(ProfileError::Io)?;
    Ok(SyncResult {
        root: root.display().to_string(),
        downloaded_files: downloads.len(),
        reused_files,
        downloaded_bytes,
        removed_files,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyMod {
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub reason: &'static str,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacySelection {
    pub path: String,
    pub sha256: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyBackup {
    pub backup_root: String,
    pub files: Vec<String>,
}

fn legacy_mods(
    root: &Path,
    manifest: &Manifest,
    owned: &Inventory,
) -> Result<Vec<LegacyMod>, ProfileError> {
    let mods = managed_target(root, "mods")?;
    let entries = match fs::read_dir(mods) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(ProfileError::Io(e)),
    };
    let mut result = Vec::new();
    for entry in entries {
        let entry = entry.map_err(ProfileError::Io)?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !name.to_ascii_lowercase().ends_with(".jar")
            || !crate::manifest::is_portable_component(&name)
        {
            continue;
        }
        let path = format!("mods/{name}");
        let expected = manifest
            .files
            .iter()
            .find(|f| f.path.eq_ignore_ascii_case(&path));
        if expected.is_some_and(|f| f.policy == FilePolicy::Seed) {
            continue;
        }
        let actual = current(root, &path)?
            .ok_or_else(|| ProfileError::Io(inventory::invalid("Mod changed during inspection")))?;
        if owned
            .files
            .get(&path)
            .is_some_and(|f| f.fingerprint == actual)
        {
            continue;
        }
        if expected.is_some_and(|f| expected_fingerprint(f) == actual) {
            continue;
        }
        let reason = if owned.files.contains_key(&path) {
            "changed_managed"
        } else if expected.is_some() {
            "conflicts_with_pack"
        } else {
            "not_in_pack"
        };
        result.push(LegacyMod {
            path,
            size: actual.size,
            sha256: actual.sha256,
            reason,
        });
    }
    result.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}

/// Informational list, not ownership evidence. Unknown extra mods remain in
/// place; callers must show the file/hash and obtain an explicit selection.
pub fn list_legacy_mods(root: &Path, manifest: &Manifest) -> Result<Vec<LegacyMod>, ProfileError> {
    let store = Store::open(root).map_err(ProfileError::Io)?;
    if store.pending().map_err(ProfileError::Io)? {
        return Err(ProfileError::Io(inventory::invalid(
            "Finish recovery before reviewing legacy mods",
        )));
    }
    legacy_mods(root, manifest, &store.load().map_err(ProfileError::Io)?)
}

/// Moves only currently listed, explicitly chosen JARs with the reviewed hash.
/// No arbitrary local path, ownership adoption, seed or personal data cleanup.
pub fn backup_legacy_mods(
    root: &Path,
    manifest: &Manifest,
    selections: &[LegacySelection],
) -> Result<LegacyBackup, ProfileError> {
    if selections.is_empty() {
        return Err(ProfileError::Io(inventory::invalid(
            "Select at least one reviewed mod",
        )));
    }
    let store = Store::open(root).map_err(ProfileError::Io)?;
    store.recover().map_err(ProfileError::Io)?;
    let owned = store.load().map_err(ProfileError::Io)?;
    let candidates = legacy_mods(root, manifest, &owned)?;
    let mut unique = BTreeSet::new();
    let mut changes = Vec::new();
    for selection in selections {
        if !unique.insert(selection.path.clone()) {
            return Err(ProfileError::Io(inventory::invalid(
                "Duplicate selected mod",
            )));
        }
        let candidate = candidates
            .iter()
            .find(|c| c.path == selection.path && c.sha256 == selection.sha256)
            .ok_or_else(|| {
                ProfileError::Io(inventory::invalid(
                    "Selected mod changed or is no longer eligible; review the list again",
                ))
            })?;
        changes.push(Change {
            path: candidate.path.clone(),
            before: Some(Fingerprint {
                size: candidate.size,
                sha256: candidate.sha256.clone(),
            }),
            after: None,
        });
    }
    let transaction = store.transaction().map_err(ProfileError::Io)?;
    let backup_root = store
        .root
        .join(&transaction)
        .join("backup")
        .display()
        .to_string();
    // Retain a path-to-index map alongside backups after the journal completes.
    let map = serde_json::to_vec(&selections.iter().map(|s| &s.path).collect::<Vec<_>>())
        .map_err(|e| ProfileError::Io(inventory::invalid(e.to_string())))?;
    crate::storage::write_atomic(&store.root.join(&transaction).join("files.json"), &map)
        .map_err(ProfileError::Io)?;
    let mut next = owned;
    for selection in selections {
        next.files.remove(&selection.path);
    }
    store
        .prepare(transaction, changes, next)
        .map_err(ProfileError::Io)?;
    store.recover().map_err(ProfileError::Io)?;
    Ok(LegacyBackup {
        backup_root,
        files: selections.iter().map(|s| s.path.clone()).collect(),
    })
}

fn managed_target(root: &Path, relative: &str) -> Result<PathBuf, ProfileError> {
    if inventory::protected(relative) {
        return Err(ProfileError::UnsafePath(root.join(relative)));
    }
    inventory::checked_path(root, relative)
        .map_err(|_| ProfileError::UnsafePath(root.join(relative)))
}

fn download_managed_file(
    client: &Client,
    expected: &ManagedFile,
    target: &Path,
) -> Result<u64, ProfileError> {
    download::download_verified(
        client,
        &expected.url,
        target,
        Some(expected.size),
        &Checksum::Sha256(expected.sha256.clone()),
        |_, _| {},
    )
    .map_err(|source| ProfileError::Download {
        path: expected.path.clone(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::inspect;
    use crate::manifest::{FilePolicy, Loader, ManagedFile, Manifest, Minecraft};
    #[cfg(test)]
    use sha2::{Digest, Sha256};
    use std::{
        fs, process,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn trusted_temporary_root() -> std::path::PathBuf {
        // Resolve only the OS-provided fixture anchor. The profile root, its
        // parent and payload descendants retain their production link checks.
        std::env::temp_dir().canonicalize().unwrap()
    }

    fn manifest(hash: String, size: u64) -> Manifest {
        Manifest {
            schema_version: 1,
            id: "aeronautics".into(),
            display_name: "Aeronautics".into(),
            minecraft: Minecraft {
                version: "1.21.1".into(),
                loader: Loader {
                    kind: "neoforge".into(),
                    version: "21.1.248".into(),
                },
                java_major: 21,
            },
            files: vec![ManagedFile {
                path: "mods/example.jar".into(),
                url: "https://cdn.shacraft.ru/example.jar".into(),
                sha256: hash,
                size,
                policy: FilePolicy::Managed,
            }],
        }
    }

    #[test]
    fn reports_missing_and_matching_files() {
        let root = trusted_temporary_root().join(format!(
            "shacraft-launcher-test-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let bytes = b"ShaCraft test file";
        let digest = format!("{:x}", Sha256::digest(bytes));
        let expected = manifest(digest, bytes.len() as u64);

        let missing = inspect(&root, &expected).unwrap();
        assert_eq!(missing.missing_files, 1);
        assert!(!missing.up_to_date);

        fs::create_dir_all(root.join("mods")).unwrap();
        fs::write(root.join("mods/example.jar"), bytes).unwrap();
        let current = inspect(&root, &expected).unwrap();
        assert_eq!(current.missing_files, 0);
        assert_eq!(current.mismatched_files, 0);
        assert!(current.up_to_date);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn edited_seed_files_remain_up_to_date() {
        let root = trusted_temporary_root().join(format!(
            "shacraft-seed-test-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("mods")).unwrap();
        fs::write(root.join("mods/example.jar"), b"player's edits").unwrap();
        let mut expected = manifest("0".repeat(64), 42);
        expected.files[0].policy = FilePolicy::Seed;
        assert!(inspect(&root, &expected).unwrap().up_to_date);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn preserves_changed_seed_files_as_current() {
        let root = trusted_temporary_root().join(format!(
            "shacraft-launcher-seed-test-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut expected = manifest("0".repeat(64), 42);
        expected.files[0].policy = FilePolicy::Seed;
        fs::create_dir_all(root.join("mods")).unwrap();
        fs::write(root.join("mods/example.jar"), b"player customization").unwrap();
        let inspection = inspect(&root, &expected).unwrap();
        assert_eq!(inspection.missing_files, 0);
        assert_eq!(inspection.mismatched_files, 0);
        assert!(inspection.up_to_date);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn refuses_linked_profile_directories() {
        use std::os::unix::fs::symlink;
        let root = trusted_temporary_root().join(format!(
            "shacraft-link-test-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("outside")).unwrap();
        fs::create_dir_all(root.join("profile")).unwrap();
        symlink(root.join("outside"), root.join("profile/mods")).unwrap();
        assert!(matches!(
            inspect(&root.join("profile"), &manifest("0".repeat(64), 42)),
            Err(super::ProfileError::UnsafePath(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod update_tests {
    use super::*;
    use crate::manifest::{Loader, Minecraft};
    use std::{
        collections::BTreeMap,
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        base: PathBuf,
        root: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let base = std::env::temp_dir().join(format!(
                "shacraft-update-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let root = base.join("profiles/aeronautics");
            fs::create_dir_all(&root).unwrap();
            Self { base, root }
        }
        fn write(&self, path: &str, bytes: &[u8]) {
            let target = self.root.join(path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, bytes).unwrap();
        }
        fn bytes(&self, path: &str) -> Vec<u8> {
            fs::read(self.root.join(path)).unwrap()
        }
        fn sync(
            &self,
            manifest: &Manifest,
            files: &[(&str, &[u8])],
        ) -> Result<SyncResult, ProfileError> {
            let content: BTreeMap<_, _> = files.iter().copied().collect();
            sync_with(&self.root, manifest, |file, target| {
                let bytes = content
                    .get(file.path.as_str())
                    .expect("unexpected download");
                fs::write(target, bytes).map_err(ProfileError::Io)?;
                Ok(bytes.len() as u64)
            })
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.base).unwrap();
        }
    }
    fn pack(files: &[(&str, &[u8], FilePolicy)]) -> Manifest {
        Manifest {
            schema_version: 1,
            id: "aeronautics".into(),
            display_name: "Aeronautics".into(),
            minecraft: Minecraft {
                version: "1.21.1".into(),
                loader: Loader {
                    kind: "neoforge".into(),
                    version: "21.1.248".into(),
                },
                java_major: 21,
            },
            files: files
                .iter()
                .map(|(path, bytes, policy)| ManagedFile {
                    path: (*path).into(),
                    url: format!("https://cdn.shacraft.ru/{path}"),
                    sha256: format!("{:x}", Sha256::digest(bytes)),
                    size: bytes.len() as u64,
                    policy: *policy,
                })
                .collect(),
        }
    }

    #[test]
    fn renamed_owned_mod_is_backed_up_and_user_mod_and_seed_survive() {
        let f = Fixture::new();
        let a = pack(&[
            ("mods/one-1.jar", b"old", FilePolicy::Managed),
            ("config/seed.txt", b"default", FilePolicy::Seed),
        ]);
        f.sync(
            &a,
            &[("mods/one-1.jar", b"old"), ("config/seed.txt", b"default")],
        )
        .unwrap();
        f.write("mods/personal.jar", b"mine");
        f.write("config/seed.txt", b"edits");
        let b = pack(&[("mods/one-2.jar", b"new", FilePolicy::Managed)]);
        let inspection = inspect(&f.root, &b).unwrap();
        assert_eq!(inspection.stale_files, 1);
        assert!(!inspection.up_to_date);
        let synced = f.sync(&b, &[("mods/one-2.jar", b"new")]).unwrap();
        assert_eq!(synced.removed_files, 1);
        assert!(!f.root.join("mods/one-1.jar").exists());
        assert_eq!(f.bytes("mods/one-2.jar"), b"new");
        assert_eq!(f.bytes("mods/personal.jar"), b"mine");
        assert_eq!(f.bytes("config/seed.txt"), b"edits");
        let store = Store::open(&f.root).unwrap();
        let owned = store.load().unwrap();
        assert_eq!(
            owned.files.keys().collect::<Vec<_>>(),
            vec!["mods/one-2.jar"]
        );
        let state_backups: Vec<_> = fs::read_dir(&store.root)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path().join("backup/1"))
            .filter(|p| p.exists())
            .collect();
        assert_eq!(state_backups.len(), 1);
        assert_eq!(fs::read(&state_backups[0]).unwrap(), b"old");
        assert!(inspect(&f.root, &b).unwrap().up_to_date);
    }
    #[test]
    fn missing_inventory_never_adopts_matching_or_extra_legacy_files() {
        let f = Fixture::new();
        f.write("mods/current.jar", b"pack");
        f.write("mods/old.jar", b"legacy");
        let a = pack(&[("mods/current.jar", b"pack", FilePolicy::Managed)]);
        let result = f.sync(&a, &[]).unwrap();
        assert_eq!(result.reused_files, 1);
        assert!(Store::open(&f.root)
            .unwrap()
            .load()
            .unwrap()
            .files
            .is_empty());
        f.sync(&pack(&[]), &[]).unwrap();
        assert_eq!(f.bytes("mods/current.jar"), b"pack");
        assert_eq!(f.bytes("mods/old.jar"), b"legacy");
        let list = list_legacy_mods(&f.root, &a).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].path, "mods/old.jar");
    }
    #[test]
    fn changed_owned_file_blocks_replacement_and_retirement_without_mutation() {
        let f = Fixture::new();
        let a = pack(&[("mods/current.jar", b"pack", FilePolicy::Managed)]);
        f.sync(&a, &[("mods/current.jar", b"pack")]).unwrap();
        f.write("mods/current.jar", b"player edits");
        let b = pack(&[("mods/current.jar", b"next", FilePolicy::Managed)]);
        assert!(matches!(f.sync(&b, &[]), Err(ProfileError::Conflict(_))));
        assert!(matches!(
            f.sync(&pack(&[]), &[]),
            Err(ProfileError::Conflict(_))
        ));
        assert_eq!(f.bytes("mods/current.jar"), b"player edits");
        let inspection = inspect(&f.root, &b).unwrap();
        assert!(!inspection.up_to_date);
        assert_eq!(inspection.conflicts, vec!["mods/current.jar"]);
        assert_eq!(
            list_legacy_mods(&f.root, &b).unwrap()[0].reason,
            "changed_managed"
        );
    }
    #[test]
    fn failed_staging_changes_no_payload_or_ownership() {
        let f = Fixture::new();
        let a = pack(&[("mods/current.jar", b"old", FilePolicy::Managed)]);
        f.sync(&a, &[("mods/current.jar", b"old")]).unwrap();
        let b = pack(&[
            ("mods/current.jar", b"new", FilePolicy::Managed),
            ("mods/second.jar", b"two", FilePolicy::Managed),
        ]);
        let mut downloads = 0;
        let result = sync_with(&f.root, &b, |_, path| {
            downloads += 1;
            if downloads == 2 {
                return Err(ProfileError::Io(io::Error::other("network failed")));
            }
            fs::write(path, b"new").unwrap();
            Ok(3)
        });
        assert!(result.is_err());
        assert_eq!(f.bytes("mods/current.jar"), b"old");
        assert!(!f.root.join("mods/second.jar").exists());
        assert!(!Store::open(&f.root).unwrap().pending().unwrap());
        assert!(inspect(&f.root, &a).unwrap().up_to_date);
        f.sync(
            &b,
            &[("mods/current.jar", b"new"), ("mods/second.jar", b"two")],
        )
        .unwrap();
        assert!(inspect(&f.root, &b).unwrap().up_to_date);
    }
    #[test]
    fn seed_policy_transitions_preserve_user_edits_and_do_not_adopt_seed() {
        let f = Fixture::new();
        let a = pack(&[("config/file.txt", b"old", FilePolicy::Managed)]);
        f.sync(&a, &[("config/file.txt", b"old")]).unwrap();
        f.write("config/file.txt", b"custom");
        let seeded = pack(&[("config/file.txt", b"default", FilePolicy::Seed)]);
        f.sync(&seeded, &[]).unwrap();
        assert!(Store::open(&f.root)
            .unwrap()
            .load()
            .unwrap()
            .files
            .is_empty());
        assert_eq!(f.bytes("config/file.txt"), b"custom");
        assert!(matches!(f.sync(&a, &[]), Err(ProfileError::Conflict(_))));
        f.sync(&pack(&[]), &[]).unwrap();
        assert_eq!(f.bytes("config/file.txt"), b"custom");
    }
    #[test]
    fn explicit_legacy_backup_requires_current_hash_and_preserves_every_other_file() {
        let f = Fixture::new();
        f.write("mods/old.jar", b"old");
        f.write("mods/keep.jar", b"keep");
        f.write("mods/seed.jar", b"seed edits");
        f.write("saves/world/level.dat", b"world");
        let manifest = pack(&[("mods/seed.jar", b"seed", FilePolicy::Seed)]);
        let candidates = list_legacy_mods(&f.root, &manifest).unwrap();
        assert_eq!(candidates.len(), 2);
        let old = candidates
            .iter()
            .find(|c| c.path == "mods/old.jar")
            .unwrap();
        let invalid = [LegacySelection {
            path: old.path.clone(),
            sha256: "0".repeat(64),
        }];
        assert!(backup_legacy_mods(&f.root, &manifest, &invalid).is_err());
        let traversal = [LegacySelection {
            path: "../outside.jar".into(),
            sha256: old.sha256.clone(),
        }];
        assert!(backup_legacy_mods(&f.root, &manifest, &traversal).is_err());
        let chosen = [LegacySelection {
            path: old.path.clone(),
            sha256: old.sha256.clone(),
        }];
        let backup = backup_legacy_mods(&f.root, &manifest, &chosen).unwrap();
        assert_eq!(
            fs::read(Path::new(&backup.backup_root).join("0")).unwrap(),
            b"old"
        );
        assert!(!f.root.join("mods/old.jar").exists());
        assert_eq!(f.bytes("mods/keep.jar"), b"keep");
        assert_eq!(f.bytes("mods/seed.jar"), b"seed edits");
        assert_eq!(f.bytes("saves/world/level.dat"), b"world");
        assert!(Store::open(&f.root)
            .unwrap()
            .load()
            .unwrap()
            .files
            .is_empty());
    }
    #[test]
    fn new_publication_recovers_and_retires_files_from_interrupted_previous_update() {
        let f = Fixture::new();
        let store = Store::open(&f.root).unwrap();
        let transaction = store.transaction().unwrap();
        let before = pack(&[("mods/intermediate.jar", b"middle", FilePolicy::Managed)]);
        let fingerprint = expected_fingerprint(&before.files[0]);
        let stage = store.stage(&transaction, 0).unwrap();
        fs::write(&stage, b"middle").unwrap();
        let mut next = Inventory::default();
        next.files.insert(
            "mods/intermediate.jar".into(),
            OwnedFile {
                fingerprint: fingerprint.clone(),
                snapshot: manifest_snapshot(&before),
            },
        );
        store
            .prepare(
                transaction,
                vec![Change {
                    path: "mods/intermediate.jar".into(),
                    before: None,
                    after: Some(fingerprint),
                }],
                next,
            )
            .unwrap();
        fs::create_dir_all(f.root.join("mods")).unwrap();
        fs::rename(stage, f.root.join("mods/intermediate.jar")).unwrap();
        let after = pack(&[("mods/final.jar", b"final", FilePolicy::Managed)]);
        let result = f.sync(&after, &[("mods/final.jar", b"final")]).unwrap();
        assert_eq!(result.removed_files, 1);
        assert!(!f.root.join("mods/intermediate.jar").exists());
        assert_eq!(f.bytes("mods/final.jar"), b"final");
        assert!(inspect(&f.root, &after).unwrap().up_to_date);
        assert!(!store.pending().unwrap());
    }

    #[test]
    fn unknown_collision_requires_review_before_install_and_protected_paths_are_refused() {
        let f = Fixture::new();
        f.write("mods/current.jar", b"unknown");
        let manifest = pack(&[("mods/current.jar", b"pack", FilePolicy::Managed)]);
        assert!(matches!(
            f.sync(&manifest, &[]),
            Err(ProfileError::Conflict(_))
        ));
        let list = list_legacy_mods(&f.root, &manifest).unwrap();
        backup_legacy_mods(
            &f.root,
            &manifest,
            &[LegacySelection {
                path: list[0].path.clone(),
                sha256: list[0].sha256.clone(),
            }],
        )
        .unwrap();
        f.sync(&manifest, &[("mods/current.jar", b"pack")]).unwrap();
        assert!(Store::open(&f.root)
            .unwrap()
            .load()
            .unwrap()
            .files
            .contains_key("mods/current.jar"));
        let unsafe_manifest = pack(&[("screenshots/player.png", b"bad", FilePolicy::Managed)]);
        assert!(f.sync(&unsafe_manifest, &[]).is_err());
        assert!(!f.root.join("screenshots/player.png").exists());
    }
}
