//! Process-local exclusion for operations that share installation/account files.
//!
//! Acquire before scheduling the worker and move the permit into it. Dropping
//! the caller's future cannot unlock an operation that is still running.
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};

#[derive(Default)]
pub(crate) struct LauncherOperations {
    pub lifecycle: Lifecycle,
    recovery: Mutex<Option<String>>,
    pub installation: Operation,
    pub account: Operation,
    pub shacraft_account: Operation,
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
    use super::Operation;

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
}

// Owned, Send permits are held by the worker, not the awaiting IPC future.
// Readers represent all native operations which can persist or launch; the
// sole writer represents launcher replacement, including its download stage.
#[derive(Default)]
pub(crate) struct Lifecycle(Arc<AtomicUsize>);
const EXCLUSIVE: usize = usize::MAX;
impl Lifecycle {
    pub fn shared(&self) -> Result<SharedPermit, String> {
        self.0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                if n < EXCLUSIVE - 1 {
                    Some(n + 1)
                } else {
                    None
                }
            })
            .map_err(|_| "Лаунчер обновляется. Дождитесь завершения и перезапуска.".to_string())?;
        Ok(SharedPermit(self.0.clone()))
    }
    pub fn exclusive(&self) -> Result<ExclusivePermit, String> {
        self.0
            .compare_exchange(0, EXCLUSIVE, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                "Завершите операцию с игрой, настройками или аккаунтом и повторите обновление."
                    .to_string()
            })?;
        Ok(ExclusivePermit(self.0.clone()))
    }
    pub fn is_updating(&self) -> bool {
        self.0.load(Ordering::Acquire) == EXCLUSIVE
    }
}
pub(crate) struct SharedPermit(Arc<AtomicUsize>);
impl Drop for SharedPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}
pub(crate) struct ExclusivePermit(Arc<AtomicUsize>);
impl Drop for ExclusivePermit {
    fn drop(&mut self) {
        self.0.store(0, Ordering::Release);
    }
}

impl LauncherOperations {
    /// Startup recovery is latched for this process. Removing a marker in a
    /// running application is never authority to resume writes or launch.
    pub fn latch_recovery(&self, reason: String) {
        *self.recovery.lock().unwrap() = Some(reason);
    }
    pub fn ensure_writable(&self) -> Result<(), String> {
        match self.recovery.lock().unwrap().as_ref() {
            Some(reason) => Err(reason.clone()),
            None => Ok(()),
        }
    }
}
