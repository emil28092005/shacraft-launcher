mod admission;
#[cfg(target_os = "linux")]
mod deb_updater;
mod download;
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
mod updater;

mod commands;
mod operations;

pub fn run() {
    #[cfg(target_os = "linux")]
    if let Some(code) = deb_updater::run_helper_if_requested() {
        std::process::exit(code);
    }
    use tauri::Manager;
    tauri::Builder::default()
        .manage(operations::LauncherOperations::default())
        .manage(updater::LauncherUpdater::default())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Do not interrupt the short package replacement step. A
                // download can safely be abandoned before any file changes.
                if let Some(updater) = window.try_state::<updater::LauncherUpdater>() {
                    if updater
                        .state
                        .lock()
                        .is_ok_and(|state| state.stage == updater::Stage::Installing)
                    {
                        api.prevent_close();
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::host::native_host,
            commands::host::detect_java,
            commands::host::microsoft_login_available,
            commands::host::validate_manifest,
            commands::profiles::inspect_remote_profile,
            commands::profiles::sync_remote_profile,
            commands::profiles::get_server_status,
            commands::preferences::load_settings,
            commands::preferences::save_settings,
            commands::shacraft::shacraft_authenticate,
            commands::shacraft::get_shacraft_account,
            commands::shacraft::shacraft_logout,
            commands::shacraft::shacraft_start_link,
            commands::shacraft::shacraft_claim_nickname,
            commands::shacraft::shacraft_link_status,
            commands::account::start_microsoft_login,
            commands::account::get_account,
            commands::account::logout,
            commands::game::ensure_game_installed,
            commands::game::launch_game,
            commands::updater::get_launcher_update_status,
            commands::updater::check_launcher_update,
            commands::updater::install_launcher_update,
            commands::updater::restart_launcher_after_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running ShaCraft Launcher");
}
