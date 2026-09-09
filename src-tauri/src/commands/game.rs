use super::{account::resolve_identity, data_dir};
use crate::{
    java, launch, manifest, mojang, neoforge, operations::LauncherOperations, remote, runtime,
    settings,
};
use reqwest::blocking::Client;
use serde::Serialize;
use std::{path::Path, sync::Arc, time::SystemTime};
use tauri::{AppHandle, Emitter, State};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InstallProgress {
    stage: &'static str,
    current_bytes: u64,
    total_bytes: u64,
}

/// Resolves the vanilla + (if any) loader version JSONs for `manifest` and
/// merges them, ensuring a Java runtime and (for NeoForge profiles) running
/// the installer along the way. Shared by `ensure_game_installed` and
/// `launch_game` so both always agree on exactly what "installed" means.
/// `on_progress` is forwarded to the NeoForge installer when one runs;
/// callers that don't display progress (e.g. `launch_game`, which only
/// hits this after `ensure_game_installed` already installed everything)
/// pass a no-op callback.
fn resolve_merged_version(
    client: &Client,
    manifest: &manifest::Manifest,
    java_executable: &Path,
    game_dir: &Path,
    cache_dir: &Path,
    on_progress: &mojang::ProgressCallback,
) -> Result<mojang::MergedVersion, String> {
    let mojang_manifest =
        mojang::fetch_version_manifest(client).map_err(|error| error.to_string())?;
    let vanilla_entry = mojang::find_version(&mojang_manifest, &manifest.minecraft.version)
        .ok_or_else(|| {
            format!(
                "Mojang does not list Minecraft version {}",
                manifest.minecraft.version
            )
        })?;
    let vanilla =
        mojang::fetch_version_json(client, vanilla_entry).map_err(|error| error.to_string())?;

    if manifest.minecraft.loader.kind == "neoforge" {
        let installer_client = neoforge::http_client().map_err(|error| error.to_string())?;
        let neoforge_version = neoforge::ensure_client_installed(
            &installer_client,
            java_executable,
            game_dir,
            cache_dir,
            &manifest.minecraft.loader.version,
            on_progress,
        )
        .map_err(|error| error.to_string())?;
        mojang::merge_versions(&vanilla, Some(&neoforge_version)).map_err(|error| error.to_string())
    } else {
        mojang::merge_versions(&vanilla, None).map_err(|error| error.to_string())
    }
}

/// Downloads and installs everything needed to run `profile_id`: the
/// exact Minecraft/loader version the ShaCraft-signed manifest specifies,
/// a Java runtime if none is already usable, and game assets. Emits
/// `game-install-progress` throughout with real progress for every stage:
/// download bytes for Java, installer-confirmed library/processor counts
/// for NeoForge, and download bytes for libraries/assets.
#[tauri::command]
pub(crate) async fn ensure_game_installed(
    app: AppHandle,
    state: State<'_, LauncherOperations>,
    profile_id: String,
) -> Result<(), String> {
    let game_dir = data_dir(&app)?.join("game");
    let runtime_root = game_dir.join("runtime");
    let cache_dir = game_dir.join("cache");
    let permit = state.installation.acquire("Installation")?;

    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let _permit = permit;
        let client = mojang::http_client().map_err(|error| error.to_string())?;
        let runtime_client = runtime::http_client().map_err(|error| error.to_string())?;
        let manifest = remote::fetch_manifest(&profile_id).map_err(|error| error.to_string())?;

        let stage_progress = |stage: &'static str| -> mojang::ProgressCallback {
            let app = app.clone();
            Arc::new(move |current, total| {
                let _ = app.emit(
                    "game-install-progress",
                    InstallProgress {
                        stage,
                        current_bytes: current,
                        total_bytes: total,
                    },
                );
            })
        };

        let java_install = java::ensure_java(
            &runtime_client,
            &runtime_root,
            manifest.minecraft.java_major,
            &stage_progress("java"),
        )
        .map_err(|error| error.to_string())?;

        let merged = resolve_merged_version(
            &client,
            &manifest,
            Path::new(&java_install.executable),
            &game_dir,
            &cache_dir,
            &stage_progress("neoforge"),
        )?;
        if manifest.minecraft.loader.kind != "neoforge" {
            // Vanilla-only profiles skip the installer, which normally
            // downloads vanilla itself; do it ourselves here instead.
            mojang::ensure_client_jar(
                &client,
                &game_dir,
                &merged.client_jar_version_id,
                &merged.client,
            )
            .map_err(|error| error.to_string())?;
            mojang::ensure_libraries(
                &client,
                &game_dir,
                &merged.libraries,
                &stage_progress("libraries"),
            )
            .map_err(|error| error.to_string())?;
        };

        let asset_index = mojang::ensure_asset_index(&client, &game_dir, &merged.asset_index)
            .map_err(|error| error.to_string())?;
        mojang::ensure_assets(&client, &game_dir, &asset_index, &stage_progress("assets"))
            .map_err(|error| error.to_string())?;

        Ok(())
    })
    .await
    .map_err(|error| format!("Install task failed: {error}"))?
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GameExited {
    profile_id: String,
    exit_code: Option<i32>,
}

/// Launches `profile_id` as the account chosen in settings (`account_mode`).
/// In `Microsoft` mode a real session is required (see `resolve_identity`);
/// in `Offline` mode the local nickname from settings is used, so no
/// Microsoft account is needed. Spawns the game detached; watches it on a
/// background thread only to emit `game-exited` when it eventually closes.
#[tauri::command]
pub(crate) async fn launch_game(
    app: AppHandle,
    state: State<'_, LauncherOperations>,
    profile_id: String,
) -> Result<(), String> {
    let game_dir = data_dir(&app)?.join("game");
    let data_dir = data_dir(&app)?;
    let permit = state.installation.acquire("Installation")?;
    let account_operation = state.account.clone();

    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let _permit = permit;
        let client = mojang::http_client().map_err(|error| error.to_string())?;
        let runtime_client = runtime::http_client().map_err(|error| error.to_string())?;
        let manifest = remote::fetch_manifest(&profile_id).map_err(|error| error.to_string())?;
        let profile_dir = data_dir.join("profiles").join(&manifest.id);
        let settings = settings::load(&data_dir).map_err(|error| error.to_string())?;
        let identity = resolve_identity(&data_dir, &settings, &account_operation)?;

        // Everything here should already be installed by `ensure_game_installed`,
        // so these are expected to hit their fast paths; no progress to show.
        let no_progress: mojang::ProgressCallback = Arc::new(|_, _| {});
        let java_install = java::ensure_java(
            &runtime_client,
            &game_dir.join("runtime"),
            manifest.minecraft.java_major,
            &no_progress,
        )
        .map_err(|error| error.to_string())?;
        let merged = resolve_merged_version(
            &client,
            &manifest,
            Path::new(&java_install.executable),
            &game_dir,
            &game_dir.join("cache"),
            &no_progress,
        )?;

        let timestamp = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let log_dir = data_dir.join("logs");
        std::fs::create_dir_all(&log_dir).map_err(|error| error.to_string())?;
        let log_path = log_dir.join(format!("{profile_id}-{timestamp}.log"));

        let request = launch::LaunchRequest {
            java_executable: Path::new(&java_install.executable),
            game_dir: &game_dir,
            profile_dir: &profile_dir,
            merged: &merged,
            identity: &identity,
            memory_mb: settings.memory_mb,
            log_path: &log_path,
        };
        let mut child = launch::launch(&request).map_err(|error| error.to_string())?;

        let watch_app = app.clone();
        let watch_profile_id = profile_id.clone();
        std::thread::spawn(move || {
            let exit_code = child.wait().ok().and_then(|status| status.code());
            let _ = watch_app.emit(
                "game-exited",
                GameExited {
                    profile_id: watch_profile_id,
                    exit_code,
                },
            );
        });

        Ok(())
    })
    .await
    .map_err(|error| format!("Launch task failed: {error}"))?
}
