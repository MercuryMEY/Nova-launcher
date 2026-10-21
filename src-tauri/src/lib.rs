mod account;
mod commands;
mod crash;
mod download;
mod error;
mod fsutil;
mod install;
mod java;
mod launch;
mod loader;
mod minecraft;
mod modpack;
mod mods;
mod state;

use tauri::Manager;

use state::AppState;

pub fn run() {
    // AppState::load 内部已容错：不致命的问题记录到 init_error，
    // 保证窗口一定能起来，错误通过 get_init_error 交给界面展示。
    let app_state = AppState::load();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(app_state)
        // 窗口尺寸存在配置里，所以只能在 setup 里应用：
        // tauri.conf.json 是编译期常量，改不了运行时值
        .setup(|app| {
            let cfg = app.state::<AppState>().snapshot();
            commands::apply_window(app.handle(), &cfg.settings);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::get_app_info,
            commands::get_init_error,
            commands::get_config,
            commands::save_settings,
            commands::open_external,
            commands::version::list_remote_versions,
            commands::version::list_installed_versions,
            commands::version::preview_version_args,
            commands::account::list_accounts,
            commands::account::add_offline_account,
            commands::account::update_offline_account,
            commands::account::remove_account,
            commands::account::set_current_account,
            commands::account::secret_store_status,
            commands::microsoft::ms_login_start,
            commands::microsoft::ms_login_poll,
            commands::microsoft::ms_account_state,
            commands::microsoft::ms_login_cancel,
            commands::java::list_java,
            commands::java::java_status,
            commands::java::install_java_runtime,
            commands::launch::launch_game,
            commands::launch::stop_game,
            commands::launch::game_status,
            commands::launch::list_logs,
            commands::launch::read_log,
            commands::launch::delete_log,
            commands::crash::crash_analyze,
            commands::install::preview_install,
            commands::install::start_install,
            commands::install::cancel_install,
            commands::install::install_busy,
            commands::loader::loader_kinds,
            commands::loader::loader_versions,
            commands::loader::loader_install,
            commands::loader::loader_installed,
            commands::mods::mods_search,
            commands::mods::mods_versions,
            commands::mods::mods_install,
            commands::mods::mods_list,
            commands::mods::mods_dir_of,
            commands::mods::mods_toggle,
            commands::mods::mods_remove,
            commands::modpack::modpack_inspect,
            commands::modpack::modpack_import,
            commands::modpack::modpack_list,
            commands::modpack::modpack_retry,
            commands::modpack::modpack_remove,
            commands::modpack::modpack_format,
        ])
        .run(tauri::generate_context!())
        .expect("Nova Launcher 启动失败");
}
