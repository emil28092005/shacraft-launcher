//! Process-local exclusion for operations that share installation/account files.
//!
//! Acquire before scheduling the worker and move the permit into it. Dropping
//! the caller's future cannot unlock an operation that is still running.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Default)]
pub(crate) struct LauncherOperations {
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
