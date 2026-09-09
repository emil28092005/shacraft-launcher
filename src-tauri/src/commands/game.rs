use super::{data_dir, profiles::ProfileMetadata, shacraft::resolve_identity};
use crate::{
    installation_lock::InstallationLock, java, launch, manifest, mojang, neoforge,
    operations::LauncherOperations, profile, remote, runtime, session, settings, shacraft_account,
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
fn progress(app: &AppHandle, stage: &'static str) -> mojang::ProgressCallback {
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
}

fn resolve_merged_version(
    client: &Client,
    manifest: &manifest::Manifest,
    java: &Path,
    game_dir: &Path,
    cache_dir: &Path,
    on_progress: &mojang::ProgressCallback,
) -> Result<mojang::MergedVersion, String> {
    let catalog = mojang::fetch_version_manifest(client).map_err(|e| e.to_string())?;
    let entry = mojang::find_version(&catalog, &manifest.minecraft.version)
        .ok_or_else(|| format!("Mojang does not list {}", manifest.minecraft.version))?;
    let vanilla = mojang::fetch_version_json(client, entry).map_err(|e| e.to_string())?;
    // The installer may reuse an existing vanilla JAR without verifying it.
    // Establish provider integrity BEFORE any NeoForge processor uses that input.
    mojang::ensure_client_jar(
        client,
        game_dir,
        &vanilla.id,
        &vanilla
            .downloads
            .as_ref()
            .ok_or("Vanilla client download metadata is missing")?
            .client,
    )
    .map_err(|e| e.to_string())?;
    if manifest.minecraft.loader.kind == "neoforge" {
        let installer_client = neoforge::http_client().map_err(|e| e.to_string())?;
        let loader = neoforge::ensure_client_installed(
            &installer_client,
            java,
            game_dir,
            cache_dir,
            &manifest.minecraft.loader.version,
            &vanilla,
            on_progress,
        )
        .map_err(|e| e.to_string())?;
        mojang::merge_versions(&vanilla, Some(&loader)).map_err(|e| e.to_string())
    } else {
        mojang::merge_versions(&vanilla, None).map_err(|e| e.to_string())
    }
}

/// Internal stages receive the same verified snapshot; none can refetch it.
fn prepare(
    directory: &Path,
    snapshot: &remote::VerifiedSnapshot,
    progress: &impl Fn(&'static str) -> mojang::ProgressCallback,
) -> Result<
    (
        mojang::MergedVersion,
        java::JavaInstallation,
        profile::ProfileInspection,
    ),
    String,
> {
    let manifest = &snapshot.manifest;
    let root = directory.join("profiles").join(&manifest.id);
    progress("mods")(0, 0);
    profile::sync_snapshot(&root, snapshot).map_err(|e| e.to_string())?;
    let inspection = profile::inspect(&root, manifest).map_err(|e| e.to_string())?;
    if !inspection.up_to_date {
        return Err(
            "Синхронизация не завершена; запуск остановлен. Проверьте конфликты файлов.".into(),
        );
    }
    let game_dir = directory.join("game");
    let client = mojang::http_client().map_err(|e| e.to_string())?;
    let runtime_client = runtime::http_client().map_err(|e| e.to_string())?;
    let java = java::ensure_java(
        &runtime_client,
        &game_dir.join("runtime"),
        manifest.minecraft.java_major,
        &progress("java"),
    )
    .map_err(|e| e.to_string())?;
    let merged = resolve_merged_version(
        &client,
        manifest,
        Path::new(&java.executable),
        &game_dir,
        &game_dir.join("cache"),
        &progress("neoforge"),
    )?;
    let libraries_client = mojang::library_http_client().map_err(|e| e.to_string())?;
    mojang::ensure_client_jar(
        &client,
        &game_dir,
        &merged.client_jar_version_id,
        &merged.client,
    )
    .map_err(|e| e.to_string())?;
    mojang::ensure_libraries(
        &libraries_client,
        &game_dir,
        &merged.libraries,
        &progress("libraries"),
    )
    .map_err(|e| e.to_string())?;
    let index = mojang::ensure_asset_index(&client, &game_dir, &merged.asset_index)
        .map_err(|e| e.to_string())?;
    mojang::ensure_assets(&client, &game_dir, &index, &progress("assets"))
        .map_err(|e| e.to_string())?;
    Ok((merged, java, inspection))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreparationResult {
    inspection: profile::ProfileInspection,
    metadata: ProfileMetadata,
    onboarding: Option<shacraft_account::LinkStart>,
}

/// Full repair, using one snapshot and one writer lock for mods AND the game.
#[tauri::command]
pub(crate) async fn ensure_game_installed(
    app: AppHandle,
    state: State<'_, LauncherOperations>,
    profile_id: String,
) -> Result<PreparationResult, String> {
    let directory = data_dir(&app)?;
    let lifecycle = crate::update_guard::begin_operation(&directory, &state)?;
    let permit = state.installation.acquire("Installation")?;
    tauri::async_runtime::spawn_blocking(move || {
        let _lifecycle = lifecycle;
        let _permit = permit;
        let _lock = InstallationLock::acquire(&directory)?;
        let snapshot = remote::fetch_snapshot(&profile_id).map_err(|e| e.to_string())?;
        let (_, _, inspection) = prepare(&directory, &snapshot, &|stage| progress(&app, stage))?;
        Ok(PreparationResult {
            inspection,
            metadata: (&snapshot).into(),
            onboarding: None,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GameExited {
    profile_id: String,
    exit_code: Option<i32>,
}

async fn play(
    app: AppHandle,
    state: &LauncherOperations,
    profile_id: String,
    onboarding_name: Option<String>,
) -> Result<PreparationResult, String> {
    let directory = data_dir(&app)?;
    let lifecycle = crate::update_guard::begin_operation(&directory, &state)?;
    let permit = state.installation.acquire("Installation")?;
    let account_operation = state.shacraft_account.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _lifecycle = lifecycle;
        let _permit = permit;
        let lock = InstallationLock::acquire(&directory)?;
        let snapshot = remote::fetch_snapshot(&profile_id).map_err(|e| e.to_string())?;
        if onboarding_name.is_some() && (profile_id != "aeronautics" || !snapshot.manifest.files.iter().any(|f|
            f.path.starts_with("mods/shacraft-game-bridge-") && f.path.ends_with(".jar") && f.policy == manifest::FilePolicy::Managed)) {
            return Err("Опубликованная сборка ещё не поддерживает первый вход. Нужен подписанный мод ShaCraft Game Bridge.".into());
        }
        let (merged, java, inspection) = prepare(&directory, &snapshot, &|stage| progress(&app, stage))?;
        // Refresh identity AFTER downloads; no long-lived cached permission.
        let grant = if let Some(name) = onboarding_name {
            let _account = account_operation.acquire("ShaCraft account operation")?;
            let grant = shacraft_account::start_onboarding(&directory, &name).map_err(|e| e.to_string())?;
            shacraft_account::validate_onboarding(&directory, &grant).map_err(|e| e.to_string())?;
            Some(grant)
        } else { None };
        let identity = if let Some(grant) = &grant {
            session::PlayerIdentity::Offline { name: grant.challenge.mc_username.clone() }
        } else { resolve_identity(&directory, &account_operation)? };
        let preferences = settings::load(&directory).map_err(|e| e.to_string())?;
        let logs = directory.join("logs");
        std::fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
        let timestamp = SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        let game_dir = directory.join("game");
        let profile_dir = directory.join("profiles").join(&snapshot.manifest.id);
        let log_path = logs.join(format!("{profile_id}-{timestamp}.log"));
        let request = launch::LaunchRequest { java_executable: Path::new(&java.executable), game_dir: &game_dir,
            profile_dir: &profile_dir, merged: &merged, identity: &identity, memory_mb: preferences.memory_mb,
            log_path: &log_path, onboarding_token: grant.as_ref().map(|g|g.grant_token.as_str()) };
        progress(&app, "launch")(0,0);
        lock.starting()?;
        let mut child = match launch::launch(&request) {
            Ok(child) => child,
            Err(error) => { lock.finished()?; return Err(error.to_string()); }
        };
        // Failure to record a living child, including an immediate exit,
        // terminates and waits for it before releasing the writer lock.
        if let Err(error) = lock.running(child.id()) {
            let _ = child.kill();
            if child.wait().is_ok() { let _ = lock.finished(); }
            return Err(error);
        }
        let watch_app = app.clone();
        std::thread::spawn(move || {
            let exit = child.wait();
            if exit.is_ok() { let _ = lock.finished(); }
            let _ = watch_app.emit("game-exited", GameExited {profile_id,exit_code: exit.ok().and_then(|s|s.code())});
            drop(lock);
        });
        Ok(PreparationResult { inspection, metadata: (&snapshot).into(), onboarding: grant.map(|g|g.challenge) })
    }).await.map_err(|e|e.to_string())?
}

/// Legacy command also performs the entire preparation; no public IPC can skip
/// reconciliation or substitute a fresh manifest between install and launch.
#[tauri::command]
pub(crate) async fn launch_game(
    app: AppHandle,
    state: State<'_, LauncherOperations>,
    profile_id: String,
) -> Result<PreparationResult, String> {
    play(app, &state, profile_id, None).await
}
#[tauri::command]
pub(crate) async fn launch_onboarding(
    app: AppHandle,
    state: State<'_, LauncherOperations>,
    profile_id: String,
    nickname: String,
) -> Result<PreparationResult, String> {
    if !shacraft_account::valid_nickname(&nickname) {
        return Err("Неверный игровой ник".into());
    }
    play(app, &state, profile_id, Some(nickname)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real provider downloads and installer execution; never logs in or joins
    /// a server. Artifacts stay in a fresh temporary directory for diagnosis.
    #[test]
    #[ignore = "downloads the real pack/game and executes the official installer; needs network and Java 21"]
    fn live_cold_install_and_corruption_repair() {
        let directory = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "shacraft-cold-install-{}",
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(!directory.exists());
        eprintln!("Isolated installation: {}", directory.display());
        let _lock = InstallationLock::acquire(&directory).unwrap();
        let snapshot = remote::fetch_snapshot("aeronautics").unwrap();
        let callback = |stage| -> mojang::ProgressCallback {
            eprintln!("Stage: {stage}");
            Arc::new(|_, _| {})
        };
        let (_, java, inspection) = prepare(&directory, &snapshot, &callback).unwrap();
        assert!(inspection.up_to_date);
        assert_eq!(java.major, snapshot.manifest.minecraft.java_major);
        let game = directory.join("game");
        let version = &snapshot.manifest.minecraft.loader.version;
        let json = neoforge::installed_version_json_path(&game, version);
        let jar = game.join(format!(
            "libraries/net/neoforged/neoforge/{version}/neoforge-{version}-client.jar"
        ));
        let original_json = std::fs::read(&json).unwrap();
        std::fs::write(&json, b"nonempty corrupted version JSON").unwrap();
        std::fs::write(&jar, b"nonempty corrupted patched JAR").unwrap();
        let (_, _, repaired) = prepare(&directory, &snapshot, &callback).unwrap();
        assert!(repaired.up_to_date);
        assert_eq!(std::fs::read(&json).unwrap(), original_json);
        assert!(std::fs::metadata(&jar).unwrap().len() > 1024);
        // A third preparation verifies the receipt and all downloads again.
        assert!(
            prepare(&directory, &snapshot, &callback)
                .unwrap()
                .2
                .up_to_date
        );
        eprintln!(
            "Cold install, corrupt JSON/JAR repair and healthy recheck passed: {}",
            directory.display()
        );
    }
}
