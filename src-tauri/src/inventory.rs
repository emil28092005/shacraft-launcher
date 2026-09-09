//! Launcher-owned profile state lives next to, never inside, the payload tree.
//! Callers hold the installation lock. Local records are not remote manifests:
//! they describe completed launcher writes, and never adopt an existing file.
use crate::{download, manifest::is_portable_component, storage};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_STATE_BYTES: u64 = 8 * 1024 * 1024;
static NEXT_TRANSACTION: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct Fingerprint {
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct OwnedFile {
    pub fingerprint: Fingerprint,
    pub snapshot: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Inventory {
    pub version: u32,
    pub files: BTreeMap<String, OwnedFile>,
}
impl Default for Inventory {
    fn default() -> Self {
        Self {
            version: 1,
            files: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Change {
    pub path: String,
    pub before: Option<Fingerprint>,
    pub after: Option<Fingerprint>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Journal {
    version: u32,
    pub transaction: String,
    pub changes: Vec<Change>,
    pub next: Inventory,
}

pub(crate) struct Store {
    pub root: PathBuf,
    profile: PathBuf,
}

pub(crate) fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

pub(crate) fn safe_relative(relative: &str) -> bool {
    !relative.is_empty() && relative.split('/').all(is_portable_component)
}

// Personal game data is never eligible for automated profile management.
pub(crate) fn protected(relative: &str) -> bool {
    matches!(
        relative
            .split('/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "saves" | "worlds" | "screenshots" | "logs" | "crash-reports"
    )
}

pub(crate) fn checked_path(root: &Path, relative: &str) -> io::Result<PathBuf> {
    if !safe_relative(relative) {
        return Err(invalid("Unsafe stored profile path"));
    }
    reject_links(root)?;
    let mut path = root.to_path_buf();
    for component in relative.split('/') {
        path.push(component);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(invalid("Profile path contains a symbolic link"))
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(path)
}

fn reject_links(path: &Path) -> io::Result<()> {
    // The caller supplies the trusted profile/state root. Check that root and
    // its immediate parent; checked_path walks every descendant separately.
    // System ancestors may legitimately be links (e.g. /var on macOS).
    for ancestor in path.ancestors().take(2) {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(invalid(
                    "Launcher state/profile path contains a symbolic link",
                ))
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

pub(crate) fn fingerprint(path: &Path) -> io::Result<Option<Fingerprint>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(invalid("Expected a regular profile file"));
    }
    Ok(Some(Fingerprint {
        size: metadata.len(),
        sha256: download::file_hashes(path)?.1,
    }))
}

fn valid_fingerprint(value: &Fingerprint) -> bool {
    value.sha256.len() == 64 && value.sha256.bytes().all(|b| b.is_ascii_hexdigit())
}
fn validate_inventory(inventory: &Inventory) -> io::Result<()> {
    if inventory.version != 1
        || inventory.files.iter().any(|(path, record)| {
            !safe_relative(path)
                || protected(path)
                || !valid_fingerprint(&record.fingerprint)
                || record.snapshot.len() != 64
                || !record.snapshot.bytes().all(|b| b.is_ascii_hexdigit())
        })
    {
        return Err(invalid(
            "Invalid launcher ownership inventory; existing files were preserved",
        ));
    }
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> io::Result<Option<T>> {
    reject_links(path)?;
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut bytes = Vec::new();
    file.take(MAX_STATE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err(invalid("Launcher state is too large"));
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| invalid("Invalid launcher state; existing files were preserved"))
}

impl Store {
    pub fn open(profile: &Path) -> io::Result<Self> {
        reject_links(profile)?;
        let name = profile
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|s| is_portable_component(s))
            .ok_or_else(|| invalid("Invalid profile directory"))?;
        let parent = profile
            .parent()
            .ok_or_else(|| invalid("Profile needs a parent directory"))?;
        let root = parent.join(format!(".{name}.shacraft-state"));
        reject_links(&root)?;
        Ok(Self {
            root,
            profile: profile.to_path_buf(),
        })
    }
    pub fn load(&self) -> io::Result<Inventory> {
        let inventory = read_json(&self.root.join("inventory.json"))?.unwrap_or_default();
        validate_inventory(&inventory)?;
        Ok(inventory)
    }
    pub fn pending(&self) -> io::Result<bool> {
        Ok(self.read_journal()?.is_some())
    }
    fn read_journal(&self) -> io::Result<Option<Journal>> {
        let journal: Option<Journal> = read_json(&self.root.join("pending.json"))?;
        if let Some(journal) = &journal {
            validate_inventory(&journal.next)?;
            let mut paths = std::collections::HashSet::new();
            if journal.version != 1
                || !is_portable_component(&journal.transaction)
                || !journal.transaction.starts_with("tx-")
                || journal.changes.iter().any(|change| {
                    !safe_relative(&change.path)
                        || protected(&change.path)
                        || !paths.insert(change.path.to_lowercase())
                        || change
                            .before
                            .as_ref()
                            .is_some_and(|f| !valid_fingerprint(f))
                        || change.after.as_ref().is_some_and(|f| !valid_fingerprint(f))
                })
            {
                return Err(invalid(
                    "Invalid pending update; existing files were preserved",
                ));
            }
        }
        Ok(journal)
    }
    pub fn transaction(&self) -> io::Result<String> {
        reject_links(&self.root)?;
        fs::create_dir_all(&self.root)?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let name = format!(
            "tx-{timestamp}-{}-{}",
            std::process::id(),
            NEXT_TRANSACTION.fetch_add(1, Ordering::Relaxed)
        );
        fs::create_dir(self.root.join(&name))?;
        fs::create_dir(self.root.join(&name).join("staged"))?;
        fs::create_dir(self.root.join(&name).join("backup"))?;
        Ok(name)
    }
    pub fn stage(&self, transaction: &str, index: usize) -> io::Result<PathBuf> {
        checked_path(&self.root, &format!("{transaction}/staged/{index}"))
    }
    pub fn prepare(
        &self,
        transaction: String,
        changes: Vec<Change>,
        next: Inventory,
    ) -> io::Result<()> {
        if self.pending()? {
            return Err(invalid("A previous profile update needs recovery"));
        }
        validate_inventory(&next)?;
        let journal = Journal {
            version: 1,
            transaction,
            changes,
            next,
        };
        let bytes = serde_json::to_vec(&journal).map_err(|e| invalid(e.to_string()))?;
        if bytes.len() as u64 > MAX_STATE_BYTES {
            return Err(invalid("Profile update journal is too large"));
        }
        let transaction_root = checked_path(&self.root, &journal.transaction)?;
        storage::write_atomic(&transaction_root.join("receipt.json"), &bytes)?;
        sync_directory(&transaction_root.join("staged"))?;
        sync_directory(&transaction_root)?;
        storage::write_atomic(&self.root.join("pending.json"), &bytes)?;
        sync_directory(&self.root)
    }
    /// Roll forward only when every affected path still matches its before or
    /// after image. Staged bytes are rehashed; unexpected local changes stop
    /// recovery without overwriting them. Backups remain available to the user.
    pub fn recover(&self) -> io::Result<()> {
        let Some(journal) = self.read_journal()? else {
            return Ok(());
        };
        // Check every transition first, before moving any remaining file.
        for (index, change) in journal.changes.iter().enumerate() {
            self.check_change(&journal.transaction, index, change)?;
        }
        for (index, change) in journal.changes.iter().enumerate() {
            self.apply_change(&journal.transaction, index, change)?;
        }
        let bytes = serde_json::to_vec(&journal.next).map_err(|e| invalid(e.to_string()))?;
        storage::write_atomic(&self.root.join("inventory.json"), &bytes)?;
        sync_directory(&self.root)?;
        fs::remove_file(self.root.join("pending.json"))?;
        sync_directory(&self.root)
    }
    fn check_change(&self, transaction: &str, index: usize, change: &Change) -> io::Result<()> {
        let target = checked_path(&self.profile, &change.path)?;
        let actual = fingerprint(&target)?;
        let backup = checked_path(&self.root, &format!("{transaction}/backup/{index}"))?;
        let saved = fingerprint(&backup)?;
        if actual == change.after && (change.before.is_none() || saved == change.before) {
            // A consumed stage proves that the launcher completed the rename.
            // If it is still present, identical bytes may have been created by
            // the user during download; do not silently adopt that file.
            if change.after.is_some() && fingerprint(&self.stage(transaction, index)?)?.is_some() {
                return Err(invalid(format!(
                    "Update conflict: {} appeared during staging; file preserved",
                    change.path
                )));
            }
            return Ok(());
        }
        if actual != change.before && !(actual.is_none() && saved == change.before) {
            return Err(invalid(format!(
                "Update conflict: {} changed; file preserved",
                change.path
            )));
        }
        if actual.is_some() && saved.is_some() {
            return Err(invalid(format!(
                "Update conflict: {} and its backup both exist",
                change.path
            )));
        }
        if let Some(after) = &change.after {
            if fingerprint(&self.stage(transaction, index)?)?.as_ref() != Some(after) {
                return Err(invalid(format!(
                    "Staged file is missing or corrupt: {}; retry needs recovery",
                    change.path
                )));
            }
        }
        Ok(())
    }
    fn apply_change(&self, transaction: &str, index: usize, change: &Change) -> io::Result<()> {
        self.check_change(transaction, index, change)?;
        let target = checked_path(&self.profile, &change.path)?;
        let actual = fingerprint(&target)?;
        let backup = checked_path(&self.root, &format!("{transaction}/backup/{index}"))?;
        if actual == change.after {
            return Ok(());
        }
        if actual.is_some() {
            fs::rename(&target, &backup)?;
            sync_directory(target.parent().unwrap())?;
            sync_directory(backup.parent().unwrap())?;
        }
        if change.after.is_some() {
            fs::create_dir_all(target.parent().unwrap())?;
            fs::rename(self.stage(transaction, index)?, &target)?;
            sync_directory(target.parent().unwrap())?;
        }
        Ok(())
    }
}

fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        fs::File::open(path)?.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    struct Fixture {
        base: PathBuf,
        profile: PathBuf,
        store: Store,
    }
    impl Fixture {
        fn new() -> Self {
            let base = std::env::temp_dir().join(format!(
                "shacraft-journal-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT_TRANSACTION.fetch_add(1, Ordering::Relaxed)
            ));
            let profile = base.join("profiles/aeronautics");
            fs::create_dir_all(profile.join("mods")).unwrap();
            let store = Store::open(&profile).unwrap();
            Self {
                base,
                profile,
                store,
            }
        }
        fn replacement(&self) -> (String, Change) {
            fs::write(self.profile.join("mods/current.jar"), b"old").unwrap();
            let transaction = self.store.transaction().unwrap();
            fs::write(self.store.stage(&transaction, 0).unwrap(), b"new").unwrap();
            let change = Change {
                path: "mods/current.jar".into(),
                before: Some(fp(b"old")),
                after: Some(fp(b"new")),
            };
            let mut next = Inventory::default();
            next.files.insert(
                change.path.clone(),
                OwnedFile {
                    fingerprint: fp(b"new"),
                    snapshot: "a".repeat(64),
                },
            );
            self.store
                .prepare(transaction.clone(), vec![change.clone()], next)
                .unwrap();
            (transaction, change)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.base).unwrap();
        }
    }
    fn fp(bytes: &[u8]) -> Fingerprint {
        Fingerprint {
            size: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(bytes)),
        }
    }

    #[test]
    fn crash_after_backing_up_old_file_finishes_replacement_and_ownership() {
        let f = Fixture::new();
        let (transaction, _) = f.replacement();
        let backup = f.store.root.join(&transaction).join("backup/0");
        fs::rename(f.profile.join("mods/current.jar"), &backup).unwrap();
        assert!(f.store.pending().unwrap());
        f.store.recover().unwrap();
        assert_eq!(
            fs::read(f.profile.join("mods/current.jar")).unwrap(),
            b"new"
        );
        assert_eq!(fs::read(backup).unwrap(), b"old");
        assert_eq!(
            f.store.load().unwrap().files["mods/current.jar"].fingerprint,
            fp(b"new")
        );
        assert!(!f.store.pending().unwrap());
        f.store.recover().unwrap(); // idempotent repeated recovery
    }
    #[test]
    fn crash_after_payload_commit_before_inventory_is_recoverable() {
        let f = Fixture::new();
        let (transaction, change) = f.replacement();
        f.store.apply_change(&transaction, 0, &change).unwrap();
        assert!(f.store.load().unwrap().files.is_empty());
        assert!(f.store.pending().unwrap());
        f.store.recover().unwrap();
        assert_eq!(f.store.load().unwrap().files.len(), 1);
        assert_eq!(
            fs::read(f.profile.join("mods/current.jar")).unwrap(),
            b"new"
        );
    }
    #[test]
    fn corrupt_staging_or_locally_changed_target_preserves_payload_and_journal() {
        let f = Fixture::new();
        let (transaction, _) = f.replacement();
        fs::write(f.store.stage(&transaction, 0).unwrap(), b"corrupt").unwrap();
        assert!(f.store.recover().is_err());
        assert_eq!(
            fs::read(f.profile.join("mods/current.jar")).unwrap(),
            b"old"
        );
        assert!(f.store.pending().unwrap());
        fs::write(f.store.stage(&transaction, 0).unwrap(), b"new").unwrap();
        fs::write(f.profile.join("mods/current.jar"), b"user").unwrap();
        assert!(f.store.recover().is_err());
        assert_eq!(
            fs::read(f.profile.join("mods/current.jar")).unwrap(),
            b"user"
        );
        assert!(f.store.pending().unwrap());
    }
    #[test]
    fn validates_all_transitions_before_resuming_any_remaining_move() {
        let f = Fixture::new();
        fs::write(f.profile.join("mods/first.jar"), b"first").unwrap();
        fs::write(f.profile.join("mods/second.jar"), b"second").unwrap();
        let transaction = f.store.transaction().unwrap();
        let changes = vec![
            Change {
                path: "mods/first.jar".into(),
                before: Some(fp(b"first")),
                after: None,
            },
            Change {
                path: "mods/second.jar".into(),
                before: Some(fp(b"second")),
                after: None,
            },
        ];
        f.store
            .prepare(transaction, changes, Inventory::default())
            .unwrap();
        fs::write(f.profile.join("mods/second.jar"), b"edits").unwrap();
        assert!(f.store.recover().is_err());
        assert_eq!(
            fs::read(f.profile.join("mods/first.jar")).unwrap(),
            b"first"
        );
        assert_eq!(
            fs::read(f.profile.join("mods/second.jar")).unwrap(),
            b"edits"
        );
    }
    #[test]
    fn pending_transaction_prevents_ready_even_if_current_manifest_files_match() {
        let f = Fixture::new();
        let (transaction, change) = f.replacement();
        f.store.apply_change(&transaction, 0, &change).unwrap();
        let manifest = crate::manifest::validate_json(&format!(r#"{{"schemaVersion":1,"id":"aeronautics","displayName":"Test","minecraft":{{"version":"1.21.1","loader":{{"kind":"neoforge","version":"21.1.248"}},"javaMajor":21}},"files":[{{"path":"mods/current.jar","url":"https://cdn.shacraft.ru/current.jar","size":3,"sha256":"{}","policy":"managed"}}]}}"#, fp(b"new").sha256)).unwrap();
        let inspection = crate::profile::inspect(&f.profile, &manifest).unwrap();
        assert!(inspection.pending_update);
        assert!(!inspection.up_to_date);
        f.store.recover().unwrap();
        assert!(
            crate::profile::inspect(&f.profile, &manifest)
                .unwrap()
                .up_to_date
        );
    }
    #[test]
    fn matching_file_created_during_staging_is_not_adopted() {
        let f = Fixture::new();
        let transaction = f.store.transaction().unwrap();
        fs::write(f.store.stage(&transaction, 0).unwrap(), b"new").unwrap();
        let mut next = Inventory::default();
        next.files.insert(
            "mods/new.jar".into(),
            OwnedFile {
                fingerprint: fp(b"new"),
                snapshot: "a".repeat(64),
            },
        );
        f.store
            .prepare(
                transaction,
                vec![Change {
                    path: "mods/new.jar".into(),
                    before: None,
                    after: Some(fp(b"new")),
                }],
                next,
            )
            .unwrap();
        fs::write(f.profile.join("mods/new.jar"), b"new").unwrap();
        assert!(f.store.recover().is_err());
        assert!(f.store.load().unwrap().files.is_empty());
        assert_eq!(fs::read(f.profile.join("mods/new.jar")).unwrap(), b"new");
        assert!(f.store.pending().unwrap());
    }

    #[test]
    fn corrupt_or_unsafe_inventory_fails_closed_without_adoption() {
        let f = Fixture::new();
        f.store.transaction().unwrap();
        fs::write(f.store.root.join("inventory.json"), b"broken JSON").unwrap();
        assert!(f.store.load().is_err());
        let mut inventory = Inventory::default();
        inventory.files.insert(
            "../outside.jar".into(),
            OwnedFile {
                fingerprint: fp(b"outside"),
                snapshot: "a".repeat(64),
            },
        );
        fs::write(
            f.store.root.join("inventory.json"),
            serde_json::to_vec(&inventory).unwrap(),
        )
        .unwrap();
        assert!(f.store.load().is_err());
    }
    #[cfg(unix)]
    #[test]
    fn linked_state_and_payload_are_refused() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        let outside = f.base.join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, &f.store.root).unwrap();
        assert!(Store::open(&f.profile).is_err());
        fs::remove_file(&f.store.root).unwrap();
        symlink(&outside, f.profile.join("mods/linked.jar")).unwrap();
        assert!(checked_path(&f.profile, "mods/linked.jar").is_err());
    }
}
