mod platform;
mod monitor;
mod browser;
mod cdp;
mod duplicate;
mod tasks;
mod actions;
mod search;
mod db;

use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            // Initialize database under the OS app-data dir (%APPDATA%/<identifier>)
            let data_dir = app.path().app_data_dir().ok();
            tauri::async_runtime::block_on(async {
                db::init_db(data_dir).await.expect("Failed to initialize database");
            });

            // WebSocket server for the browser extension (tab-level data
            // from normally-running browsers; no debug flag needed)
            browser::start_extension_server(app.handle().clone());

            // System-wide Ctrl+Shift+F toggles the search overlay.
            // The webview listens for the "toggle-search" event. Registration
            // failure (hotkey already taken) only logs — the UI keeps working.
            use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
            if let Err(e) = app.global_shortcut().on_shortcut("ctrl+shift+f", |app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    let _ = app.emit("toggle-search", ());
                }
            }) {
                eprintln!("Failed to register global shortcut Ctrl+Shift+F: {}", e);
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            monitor::get_all_windows,
            monitor::get_tracked_items,
            cdp::check_cdp_status,
            browser::get_extension_status,
            duplicate::detect_duplicates,
            duplicate::close_duplicates,
            actions::focus_window,
            actions::close_window,
            actions::launch_browser_debug,
            search::search_items,
            tasks::get_all_tasks,
            tasks::create_task,
            tasks::update_task,
            tasks::delete_task,
            tasks::assign_item_to_task,
            tasks::unassign_item_from_task,
            tasks::get_task_items,
            db::get_stats,
        ])
        .run(tauri::generate_context!())
        .expect("error while running TabFlow");
}
