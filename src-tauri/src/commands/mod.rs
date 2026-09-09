//! Thin Tauri adapters grouped by the domain they expose.
pub(crate) mod account;
pub(crate) mod game;
pub(crate) mod host;
pub(crate) mod preferences;
pub(crate) mod profiles;
pub(crate) mod shacraft;
pub(crate) mod updater;

use std::path::PathBuf;
use tauri::{AppHandle, Manager};

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|error| format!("Cannot resolve launcher data directory: {error}"))
}
