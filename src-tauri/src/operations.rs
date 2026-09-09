//! Process-local exclusion for operations that share installation/account files.
//!
//! Acquire before scheduling the worker and move the permit into it. Dropping
//! the caller's future cannot unlock an operation that is still running.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone, Default)]
pub(crate) struct LauncherOperations {
    pub installation: Operation,
    pub account: Operation,
    pub shacraft_account: Operation,
    // Held from launch scheduling until Child::wait completes, not just spawn.
    pub game: Operation,
}

impl LauncherOperations {
    /// Acquire every mutation gate without waiting. Partial acquisition is
    /// rolled back by RAII, so a failed update cannot strand an account gate.
    pub fn acquire_update(&self) -> Result<UpdatePermits, String> {
        let installation = self.installation.acquire("Установка игры")?;
        let account = self.account.acquire("Вход в аккаунт")?;
        let shacraft_account = self
            .shacraft_account
            .acquire("Операция с аккаунтом ShaCraft")?;
        let game = self
            .game
            .acquire("Игра")
            .map_err(|_| "Закройте Minecraft перед обновлением лаунчера.".to_string())?;
        Ok(UpdatePermits {
            _installation: installation,
            _account: account,
            _shacraft_account: shacraft_account,
            _game: game,
        })
    }
}

pub(crate) struct UpdatePermits {
    _installation: Permit,
    _account: Permit,
    _shacraft_account: Permit,
    _game: Permit,
}

#[derive(Clone, Default)]
pub(crate) struct Operation(Arc<AtomicBool>);

impl Operation {
    pub fn acquire(&self, label: &str) -> Result<Permit, String> {
        self.0
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .map_err(|_| format!("{label} is already in progress; wait for it to finish"))?;
        Ok(Permit(self.0.clone()))
    }
}

pub(crate) struct Permit(Arc<AtomicBool>);

impl Drop for Permit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::{LauncherOperations, Operation};

    #[test]
    fn rejects_overlap_and_releases_on_worker_error() {
        let operation = Operation::default();
        let worker = || -> Result<(), String> {
            let _permit = operation.acquire("Installation")?;
            assert!(operation.acquire("Installation").is_err());
            Err("simulated worker failure".into())
        };
        assert!(worker().is_err());
        assert!(operation.acquire("Installation").is_ok());
    }

    #[test]
    fn running_game_blocks_update_and_partial_locks_are_released() {
        let operations = LauncherOperations::default();
        let game = operations.game.acquire("game").unwrap();
        assert!(operations.acquire_update().is_err());
        assert!(operations.installation.acquire("install").is_ok());
        assert!(operations.account.acquire("account").is_ok());
        assert!(operations
            .shacraft_account
            .acquire("ShaCraft account")
            .is_ok());
        drop(game);
        assert!(operations.acquire_update().is_ok());
    }

    #[test]
    fn update_excludes_game_and_accounts_until_permit_drop() {
        let operations = LauncherOperations::default();
        let permit = operations.acquire_update().unwrap();
        assert!(operations.installation.acquire("install").is_err());
        assert!(operations.account.acquire("account").is_err());
        assert!(operations
            .shacraft_account
            .acquire("ShaCraft account")
            .is_err());
        assert!(operations.game.acquire("game").is_err());
        assert!(operations.acquire_update().is_err());
        drop(permit);
        assert!(operations.acquire_update().is_ok());
    }

    #[test]
    fn account_operation_blocks_update_without_stranding_installation() {
        let operations = LauncherOperations::default();
        let _account = operations.shacraft_account.acquire("account").unwrap();
        assert!(operations.acquire_update().is_err());
        assert!(operations.installation.acquire("install").is_ok());
    }
}
