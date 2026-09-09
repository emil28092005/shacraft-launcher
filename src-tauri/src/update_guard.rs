//! Launcher replacement is a distinct trust and lifecycle boundary from game
//! installation. A process-local gate drains all native writes; the game lock
//! also checks the durable detached-game lease. A handoff record survives the
//! Windows updater's immediate process exit and is never cleared by a timeout.
use crate::{
    installation_lock::InstallationLock,
    operations::{ExclusivePermit, LauncherOperations, Permit},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
};

// A renamed durable marker must survive a power loss before installer handoff.
// Unix directory fsync persists the name itself, in addition to write_atomic's
// fsync of the file contents. Windows uses its native file replacement semantics.
fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        File::open(path)
            .and_then(|file| file.sync_all())
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

const RECOVERY: &str = "Предыдущая установка обновления не завершена или её запись повреждена. Автоматическое продолжение заблокировано. Закройте игру, установщик и лаунчер, затем восстановите приложение официальным пакетом того же типа. Настройки и игровые файлы удалять не нужно.";

fn ordinary(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            Err("Служебный путь обновления является ссылкой".into())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn directory(data: &Path) -> Result<PathBuf, String> {
    ordinary(data)?;
    fs::create_dir_all(data).map_err(|e| e.to_string())?;
    let path = data.join("launcher-state");
    ordinary(&path)?;
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    sync_directory(data)?;
    if let Some(parent) = data.parent() {
        sync_directory(parent)?;
    }
    Ok(path)
}

/// Retained in Tauri state for the entire process lifetime. In particular an
/// idle second launcher cannot keep an old executable loaded during replacement.
pub(crate) struct InstanceGuard {
    _file: File,
    recovery_reason: Option<String>,
}
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        // Release the owner lock even if a concurrent spawn retains a temporary
        // inherited file description before exec. Never clear the handoff marker.
        let _ = self._file.unlock();
    }
}

impl InstanceGuard {
    pub fn recovery_reason(&self) -> Option<String> {
        self.recovery_reason.clone()
    }
    pub fn acquire(data: &Path, current_version: &str) -> Result<Self, String> {
        let dir = directory(data)?;
        let path = dir.join("instance.lock");
        ordinary(&path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path).map_err(|e| e.to_string())?;
        let marker = dir.join("pending-update.json");
        let mut acquired = file.try_lock().is_ok();
        if !acquired && read_pending(&marker)?.is_some_and(|pending| pending.to == current_version)
        {
            // Tauri starts the replacement child before its old process exits.
            // Only the exact recorded target may wait for that legitimate handoff.
            // Never remove a lock or treat elapsed time as successful acquisition.
            let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !acquired && std::time::Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(25));
                acquired = file.try_lock().is_ok();
            }
        }
        if !acquired {
            return Err(
                "ShaCraft Launcher уже запущен. Откройте его окно или завершите другой экземпляр."
                    .into(),
            );
        }
        let recovery_reason = match read_pending(&marker) {
            // A corrupt/unreadable marker becomes explicit recovery state,
            // NEVER Ready/None. Setup latches this reason before any IPC runs.
            Err(reason) => Some(reason),
            Ok(Some(pending)) if pending.to == current_version => fs::remove_file(marker)
                .map_err(|error| error.to_string())
                .and_then(|()| sync_directory(&dir))
                .err()
                .map(|error| format!("{RECOVERY} {error}")),
            Ok(Some(pending)) => Some(format!("{RECOVERY} Ожидаемая версия: {}.", pending.to)),
            Ok(None) => None,
        };
        Ok(Self {
            _file: file,
            recovery_reason,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingUpdate {
    from: String,
    to: String,
}

fn read_pending(path: &Path) -> Result<Option<PendingUpdate>, String> {
    ordinary(path)?;
    match fs::read(path) {
        Ok(bytes) if bytes.len() <= 1024 => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| RECOVERY.to_string()),
        Ok(_) => Err(RECOVERY.into()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub(crate) fn ensure_no_pending(data: &Path) -> Result<(), String> {
    if let Some(pending) = read_pending(&directory(data)?.join("pending-update.json"))? {
        return Err(format!("{RECOVERY} Ожидаемая версия: {}.", pending.to));
    }
    Ok(())
}

pub(crate) fn pending_reason(
    data: &Path,
    _current_version: &str,
) -> Result<Option<String>, String> {
    // Startup is the only place allowed to acknowledge a completed handoff.
    // Read-only status must never remove a marker while an installer is active.
    match ensure_no_pending(data) {
        Ok(()) => Ok(None),
        Err(reason) => Ok(Some(reason)),
    }
}

pub(crate) fn begin_operation(
    data: &Path,
    operations: &LauncherOperations,
) -> Result<crate::operations::SharedPermit, String> {
    operations.ensure_writable()?;
    let permit = operations.lifecycle.shared()?;
    ensure_no_pending(data)?;
    Ok(permit)
}

pub(crate) struct UpdateGuard<'a> {
    operations: &'a LauncherOperations,
    directory: PathBuf,
    _exclusive: ExclusivePermit,
    _operation: Permit,
    _installation: InstallationLock,
}
impl<'a> UpdateGuard<'a> {
    pub fn acquire(data: &Path, operations: &'a LauncherOperations) -> Result<Self, String> {
        operations.ensure_writable()?;
        let exclusive = operations.lifecycle.exclusive()?;
        let operation = operations.installation.acquire("Обновление лаунчера")?;
        ensure_no_pending(data)?;
        let installation = InstallationLock::acquire(data)?;
        Ok(Self {
            operations,
            directory: directory(data)?,
            _exclusive: exclusive,
            _operation: operation,
            _installation: installation,
        })
    }

    /// Call only after all package checks and immediately before the platform
    /// installer. Drop intentionally preserves this record on uncertain errors.
    pub fn begin_install(&self, current_version: &str, target_version: &str) -> Result<(), String> {
        let from = semver::Version::parse(current_version).map_err(|e| e.to_string())?;
        let to = semver::Version::parse(target_version).map_err(|e| e.to_string())?;
        if to <= from || !to.pre.is_empty() || !to.build.is_empty() {
            return Err("Установка этой версии обновления запрещена".into());
        }
        let path = self.directory.join("pending-update.json");
        ordinary(&path)?;
        crate::storage::write_atomic(
            &path,
            &serde_json::to_vec(&PendingUpdate {
                from: current_version.into(),
                to: target_version.into(),
            })
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        self.operations
            .latch_recovery(format!("{RECOVERY} Ожидаемая версия: {target_version}."));
        sync_directory(&self.directory)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "shacraft-update-lock-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn instance_owner_drop_releases_lock_despite_inherited_description() {
        let data = dir();
        let owner = InstanceGuard::acquire(&data, "0.2.0").unwrap();
        let inherited = owner._file.try_clone().unwrap();
        assert!(InstanceGuard::acquire(&data, "0.2.0").is_err());
        drop(owner);
        let next = InstanceGuard::acquire(&data, "0.2.0")
            .unwrap_or_else(|error| panic!("owner instance lock remained: {error}"));
        drop(inherited);
        drop(next);
        fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn excludes_account_settings_and_game_writes_in_both_directions() {
        let dir = dir();
        let state = LauncherOperations::default();
        let write = begin_operation(&dir, &state).unwrap();
        assert!(UpdateGuard::acquire(&dir, &state).is_err());
        drop(write);
        let update = UpdateGuard::acquire(&dir, &state).unwrap();
        assert!(begin_operation(&dir, &state).is_err());
        assert!(UpdateGuard::acquire(&dir, &state).is_err());
        drop(update);
        assert!(begin_operation(&dir, &state).is_ok());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn checks_game_lease_after_launcher_has_exited() {
        let dir = dir();
        let state = LauncherOperations::default();
        let game = InstallationLock::acquire(&dir).unwrap();
        game.running(std::process::id()).unwrap();
        drop(game);
        assert!(UpdateGuard::acquire(&dir, &state).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_download_can_retry_but_installer_handoff_survives_exit() {
        let dir = dir();
        let state = LauncherOperations::default();
        drop(UpdateGuard::acquire(&dir, &state).unwrap());
        let update = UpdateGuard::acquire(&dir, &state).unwrap();
        assert!(update.begin_install("0.2.0", "0.1.1").is_err());
        assert!(update.begin_install("0.2.0", "0.2.0").is_err());
        update.begin_install("0.2.0", "0.2.1").unwrap();
        drop(update);
        assert!(begin_operation(&dir, &state).is_err());
        assert!(UpdateGuard::acquire(&dir, &state).is_err());
        let old = InstanceGuard::acquire(&dir, "0.2.0").unwrap();
        assert!(begin_operation(&dir, &state).is_err());
        drop(old);
        let different = InstanceGuard::acquire(&dir, "0.3.0").unwrap();
        assert!(begin_operation(&dir, &state).is_err());
        drop(different);
        let updated = InstanceGuard::acquire(&dir, "0.2.1").unwrap();
        assert!(begin_operation(&dir, &state).is_err()); // old process stays latched
        assert!(begin_operation(&dir, &LauncherOperations::default()).is_ok());
        drop(updated);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn target_process_waits_for_real_lock_release_during_restart() {
        let dir = dir();
        let instance = InstanceGuard::acquire(&dir, "0.2.0").unwrap();
        let state = LauncherOperations::default();
        let update = UpdateGuard::acquire(&dir, &state).unwrap();
        update.begin_install("0.2.0", "0.2.1").unwrap();
        drop(update);
        // Model Tauri's spawn-before-exit with an OS lock held by the old owner.
        let next_dir = dir.clone();
        let next = std::thread::spawn(move || InstanceGuard::acquire(&next_dir, "0.2.1"));
        std::thread::sleep(std::time::Duration::from_millis(75));
        assert!(!next.is_finished());
        drop(instance);
        let target = next.join().unwrap().unwrap();
        assert!(target.recovery_reason().is_none());
        assert!(begin_operation(&dir, &state).is_err());
        assert!(begin_operation(&dir, &LauncherOperations::default()).is_ok());
        drop(target);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_second_idle_instance_and_malformed_marker() {
        let dir = dir();
        let instance = InstanceGuard::acquire(&dir, "0.2.0").unwrap();
        assert!(InstanceGuard::acquire(&dir, "0.2.0").is_err());
        drop(instance);
        fs::write(
            directory(&dir).unwrap().join("pending-update.json"),
            b"invalid",
        )
        .unwrap();
        let recovery = InstanceGuard::acquire(&dir, "0.2.1").unwrap();
        let state = LauncherOperations::default();
        state.latch_recovery(
            recovery
                .recovery_reason()
                .expect("corruption must be explicit recovery"),
        );
        // Read-only diagnostics are available, while every shared write and
        // updater install remains denied, including after external deletion.
        assert!(pending_reason(&dir, "0.2.1").unwrap().is_some());
        assert!(begin_operation(&dir, &state).is_err());
        assert!(UpdateGuard::acquire(&dir, &state).is_err());
        fs::remove_file(directory(&dir).unwrap().join("pending-update.json")).unwrap();
        assert!(begin_operation(&dir, &state).is_err());
        assert!(UpdateGuard::acquire(&dir, &state).is_err());
        drop(recovery);
        fs::remove_dir_all(dir).unwrap();
    }
}
