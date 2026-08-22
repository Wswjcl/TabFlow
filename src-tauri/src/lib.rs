mod platform;
mod monitor;
mod browser;
mod cdp;
mod duplicate;
mod tasks;
mod actions;
mod search;
mod db;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // Initialize database
            tauri::async_runtime::block_on(async {
                db::init_db().await.expect("Failed to initialize database");
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            monitor::get_all_windows,
            monitor::get_tracked_items,
            cdp::check_cdp_status,
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