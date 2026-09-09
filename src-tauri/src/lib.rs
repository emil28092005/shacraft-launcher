mod download;
mod update_guard;
mod updater;
use tauri::Manager;
mod installation_lock;
mod inventory;
mod java;
mod launch;
mod manifest;
mod mojang;
mod msa;
mod neoforge;
mod profile;
mod remote;
mod runtime;
mod session;
mod settings;
mod shacraft_account;
mod storage;
mod trusted_http;

mod commands;
mod operations;

pub fn run() {
    tauri::Builder::default()
        .manage(operations::LauncherOperations::default())
        .manage(updater::UpdaterState::default())
        .plugin(
            tauri_plugin_updater::Builder::new()
                .pubkey(updater::configured_key().unwrap_or(""))
                .build(),
        )
        .setup(|app| {
            let directory = app.path().app_data_dir()?;
            let instance =
                update_guard::InstanceGuard::acquire(&directory, env!("CARGO_PKG_VERSION"))
                    .map_err(std::io::Error::other)?;
            if let Some(reason) = instance.recovery_reason() {
                app.state::<operations::LauncherOperations>()
                    .latch_recovery(reason);
            }
            app.manage(instance);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window
                    .app_handle()
                    .state::<operations::LauncherOperations>()
                    .lifecycle
                    .is_updating()
                {
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::updater::updater_status,
            commands::updater::updater_check,
            commands::updater::updater_download_install,
            commands::updater::updater_restart,
            commands::updater::updater_open_release_page,
            commands::host::native_host,
            commands::host::detect_java,
            commands::host::microsoft_login_available,
            commands::host::validate_manifest,
            commands::profiles::inspect_remote_profile,
            commands::profiles::sync_remote_profile,
            commands::profiles::get_server_status,
            commands::profiles::profile_metadata,
            commands::profiles::legacy_mods,
            commands::profiles::backup_legacy_mods,
            commands::preferences::load_settings,
            commands::preferences::save_settings,
            commands::shacraft::shacraft_authenticate,
            commands::shacraft::get_shacraft_account,
            commands::shacraft::shacraft_logout,
            commands::shacraft::shacraft_start_link,
            commands::shacraft::shacraft_link_status,
            commands::account::start_microsoft_login,
            commands::account::get_account,
            commands::account::logout,
            commands::game::ensure_game_installed,
            commands::game::launch_game,
            commands::game::launch_onboarding
        ])
        .run(tauri::generate_context!())
        .expect("error while running ShaCraft Launcher");
}
