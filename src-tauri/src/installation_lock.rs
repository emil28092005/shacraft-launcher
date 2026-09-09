//! One writer for the entire shared game tree, across launcher instances.
//! The OS lock is retained by the child watcher. A durable process lease also
//! protects a detached Minecraft after the launcher exits (PID + start time,
//! never PID alone). An interrupted spawn with no recorded child fails closed.
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
struct ProcessIdentity {
    pid: u32,
    started: u64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "state")]
enum Lease {
    Starting { launcher: ProcessIdentity },
    Running { game: ProcessIdentity },
}

fn process_identity(pid: u32) -> Result<Option<ProcessIdentity>, String> {
    let me = Pid::from_u32(std::process::id());
    let target = Pid::from_u32(pid);
    let mut system = System::new();
    // sysinfo resets each refreshed process's updated flag while removing dead
    // entries. Repeating a PID makes the second pass remove that live entry.
    let pids = if me == target {
        vec![me]
    } else {
        vec![me, target]
    };
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&pids),
        true,
        ProcessRefreshKind::nothing().without_tasks(),
    );
    // An unsupported/failed process inspection must not permit file mutation.
    if system.process(me).is_none() {
        return Err("Не удалось проверить запущенные процессы; запись файлов заблокирована".into());
    }
    system
        .process(target)
        .map(|process| {
            let started = process.start_time();
            if started == 0 {
                return Err("Не удалось определить время запуска игры".into());
            }
            Ok(ProcessIdentity { pid, started })
        })
        .transpose()
}

fn ordinary_path(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            Err("Служебный путь блокировки является ссылкой".into())
        }
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

pub(crate) struct InstallationLock {
    _file: File,
    lease: PathBuf,
}

impl Drop for InstallationLock {
    fn drop(&mut self) {
        // A concurrent Unix spawn can retain an inherited copy until exec.
        // Release this owner's lock explicitly instead of waiting for every
        // duplicate descriptor to close. Never remove the durable game lease.
        // If unlock fails, closing the file still leaves the OS fail-closed.
        let _ = self._file.unlock();
    }
}

impl InstallationLock {
    pub fn acquire(data_dir: &Path) -> Result<Self, String> {
        ordinary_path(data_dir)?;
        fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
        let directory = data_dir.join("installation-state");
        ordinary_path(&directory)?;
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let path = directory.join("writer.lock");
        ordinary_path(&path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path).map_err(|e| e.to_string())?;
        file.try_lock().map_err(|_| "Сборка используется другим экземпляром лаунчера или игрой. Закройте игру и дождитесь завершения операции.".to_string())?;
        let guard = Self {
            _file: file,
            lease: directory.join("game-lease.json"),
        };
        ordinary_path(&guard.lease)?;
        match fs::read(&guard.lease) {
            Ok(bytes) => {
                let lease: Lease = serde_json::from_slice(&bytes).map_err(|_| "Повреждена запись запущенной игры; запись файлов остановлена. Закройте Minecraft и восстановите служебную запись по инструкции.".to_string())?;
                match lease {
                    Lease::Starting { .. } => return Err("Предыдущий запуск прервался до регистрации процесса. Запись файлов заблокирована: сначала завершите Minecraft и выполните ручное восстановление game-lease.json по инструкции.".into()),
                    Lease::Running { game } => {
                        if process_identity(game.pid)?.as_ref() == Some(&game) {
                            return Err("Minecraft ещё работает. Перед обновлением или восстановлением закройте игру.".into());
                        }
                        fs::remove_file(&guard.lease).map_err(|e| e.to_string())?;
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
        Ok(guard)
    }

    fn store(&self, value: &Lease) -> Result<(), String> {
        ordinary_path(&self.lease)?;
        crate::storage::write_atomic(
            &self.lease,
            &serde_json::to_vec(value).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }

    /// Write ahead of spawn, while holding the OS lock, closing the crash window
    /// in which a child could exist with no durable evidence whatsoever.
    pub fn starting(&self) -> Result<(), String> {
        let launcher =
            process_identity(std::process::id())?.ok_or("Launcher process disappeared")?;
        self.store(&Lease::Starting { launcher })
    }

    pub fn running(&self, pid: u32) -> Result<(), String> {
        let game = process_identity(pid)?.ok_or("Игра завершилась во время запуска")?;
        self.store(&Lease::Running { game })
    }

    /// Only the owner, after failed spawn or wait() proving child termination,
    /// clears the lease. Drop intentionally does not clear it.
    pub fn finished(&self) -> Result<(), String> {
        match fs::remove_file(&self.lease) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn dir() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "shacraft-lock-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }
    #[test]
    fn separate_file_descriptions_exclude_writers() {
        let p = dir();
        let a = InstallationLock::acquire(&p).unwrap();
        assert!(InstallationLock::acquire(&p).is_err());
        drop(a);
        drop(
            InstallationLock::acquire(&p)
                .unwrap_or_else(|error| panic!("expected released lock: {error}")),
        );
        fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn live_game_lease_survives_dropping_launcher_lock() {
        let p = dir();
        let a = InstallationLock::acquire(&p).unwrap();
        a.starting().unwrap();
        a.running(std::process::id()).unwrap();
        drop(a);
        let error = InstallationLock::acquire(&p).unwrap_err_string();
        assert!(error.contains("Minecraft"), "unexpected refusal: {error}");
        fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn reused_pid_with_different_start_does_not_block_forever() {
        let p = dir();
        let a = InstallationLock::acquire(&p).unwrap();
        a.store(&Lease::Running {
            game: ProcessIdentity {
                pid: std::process::id(),
                started: 1,
            },
        })
        .unwrap();
        drop(a);
        drop(
            InstallationLock::acquire(&p)
                .unwrap_or_else(|error| panic!("expected released lock: {error}")),
        );
        fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn inherited_file_description_does_not_extend_owner_guard_lifetime() {
        let p = dir();
        let guard = InstallationLock::acquire(&p).unwrap();
        guard
            .store(&Lease::Running {
                game: ProcessIdentity {
                    pid: std::process::id(),
                    started: 1,
                },
            })
            .unwrap();
        // A concurrent Unix spawn inherits this same open file description
        // until exec closes its CLOEXEC copy. try_clone deterministically keeps
        // that description alive without relying on fork timing or sleeps.
        let inherited = guard._file.try_clone().unwrap();
        assert!(InstallationLock::acquire(&p).is_err());
        drop(guard);
        let reacquired = InstallationLock::acquire(&p)
            .unwrap_or_else(|error| panic!("owner dropped but lock remained: {error}"));
        assert!(!p.join("installation-state/game-lease.json").exists());
        drop(inherited);
        drop(reacquired);
        fs::remove_dir_all(p).unwrap();
    }

    #[test]
    fn releasing_owner_lock_keeps_live_game_lease_with_inherited_description() {
        let p = dir();
        let guard = InstallationLock::acquire(&p).unwrap();
        guard.running(std::process::id()).unwrap();
        let inherited = guard._file.try_clone().unwrap();
        drop(guard);
        let error = InstallationLock::acquire(&p).unwrap_err_string();
        assert!(error.contains("Minecraft"), "unexpected refusal: {error}");
        assert!(p.join("installation-state/game-lease.json").exists());
        drop(inherited);
        fs::remove_dir_all(p).unwrap();
    }

    #[test]
    fn interrupted_spawn_fails_closed() {
        let p = dir();
        let a = InstallationLock::acquire(&p).unwrap();
        a.starting().unwrap();
        drop(a);
        assert!(InstallationLock::acquire(&p).is_err());
        fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn current_process_identity_is_detected_without_duplicate_pid_removal() {
        let pid = std::process::id();
        let identity = process_identity(pid).unwrap().unwrap();
        assert_eq!(identity.pid, pid);
        assert!(identity.started > 0);
    }

    #[test]
    fn real_child_lease_blocks_until_child_exits_after_launcher_guard_drops() {
        const CHILD_MODE: &str = "SHACRAFT_LEASE_TEST_CHILD";
        if std::env::var_os(CHILD_MODE).is_some() {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::io::stdin().read_to_end(&mut bytes).unwrap();
            return;
        }
        // Launch this one test in child mode; stdin keeps it alive without a
        // platform shell, installed external program or arbitrary sleep.
        struct TestChild(std::process::Child);
        impl Drop for TestChild {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = TestChild(std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("installation_lock::tests::real_child_lease_blocks_until_child_exits_after_launcher_guard_drops")
            .env(CHILD_MODE, "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn().unwrap());
        let directory = dir();
        let guard = InstallationLock::acquire(&directory).unwrap();
        guard.starting().unwrap();
        guard.running(child.0.id()).unwrap();
        drop(guard);
        assert!(InstallationLock::acquire(&directory)
            .unwrap_err_string()
            .contains("Minecraft"));
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        let guard = InstallationLock::acquire(&directory).unwrap();
        assert!(!directory
            .join("installation-state/game-lease.json")
            .exists());
        drop(guard);
        fs::remove_dir_all(directory).unwrap();
    }

    trait ErrorText {
        fn unwrap_err_string(self) -> String;
    }
    impl ErrorText for Result<InstallationLock, String> {
        fn unwrap_err_string(self) -> String {
            match self {
                Err(e) => e,
                Ok(_) => panic!("expected lock refusal"),
            }
        }
    }
}
