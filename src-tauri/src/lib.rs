mod admission;
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

mod commands;
mod operations;

pub fn run() {
    tauri::Builder::default()
        .manage(operations::LauncherOperations::default())
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
            commands::game::launch_game
        ])
        .run(tauri::generate_context!())
        .expect("error while running ShaCraft Launcher");
}
