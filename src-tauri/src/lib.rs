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

/// Bring the main window back from hidden/minimized (tray restore).
fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

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
                db::init_db(data_dir.clone()).await.expect("Failed to initialize database");
            });

            // Pairing token lives next to the DB: generated once, reused on
            // every launch so paired extensions reconnect automatically.
            // On any IO failure we fall back to a per-run token (repair by
            // pasting the token once again).
            if let Some(dir) = &data_dir {
                match browser::load_or_create_token(dir) {
                    Ok(token) => browser::init_token(token),
                    Err(e) => eprintln!(
                        "Extension token: could not load/create ({e}); \
                         extensions must re-pair this run"
                    ),
                }
            }

            // WebSocket server for the browser extension (tab-level data
            // from normally-running browsers; no debug flag needed)
            browser::start_extension_server(app.handle().clone());

            // Undecorated main window: re-add WS_SYSMENU so the taskbar
            // thumbnail right-click menu and click-to-restore work.
            #[cfg(target_os = "windows")]
            if let Some(window) = app.get_webview_window("main") {
                if let Ok(hwnd) = window.hwnd() {
                    platform::restore_system_menu(hwnd.0 as isize);
                }
            }

            // Tray icon (built in code - the config only carried icon and
            // tooltip, no behavior). Left click restores the window, right
            // click offers show/quit, and closing the window hides it to
            // the tray instead of exiting the app.
            use tauri::menu::{Menu, MenuItem};
            use tauri::tray::{
                MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent,
            };
            let show = MenuItem::with_id(app, "show", "显示主界面", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            TrayIconBuilder::with_id("tabflow-tray")
                .icon(app.default_window_icon().expect("window icon").clone())
                .tooltip("TabFlow")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button, button_state, .. } = event {
                        if button == MouseButton::Left
                            && button_state == MouseButtonState::Up
                        {
                            show_main_window(tray.app_handle());
                        }
                    }
                })
                .build(app)?;

            if let Some(window) = app.get_webview_window("main") {
                let w = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = w.hide();
                    }
                });
            }

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
            db::ignore_item,
            db::unignore_resource,
            db::get_ignored_resources,
        ])
        .run(tauri::generate_context!())
        .expect("error while running TabFlow");
}
