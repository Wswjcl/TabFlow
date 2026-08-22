use crate::cdp;
use crate::db;
use crate::platform::ItemType;

/// Bring a window to the foreground (focus/jump to it)
#[tauri::command]
pub async fn focus_window(item_id: String) -> Result<(), String> {
    let items = db::get_all_tracked_items()
        .await
        .map_err(|e| e.to_string())?;

    if let Some(item) = items.iter().find(|i| i.id == item_id) {
        // Browser tab: focus via CDP
        if item.item_type == ItemType::BrowserTab {
            let ok = cdp::focus_cdp_tab(&item.id).await;
            if ok {
                let _ = db::touch_item(&item_id).await;
                return Ok(());
            }
            // CDP focus failed — fall back to focusing the browser's main window
            if let Some(hwnd) = find_browser_window_handle(&item.process_name) {
                focus_window_by_handle(hwnd);
                let _ = db::touch_item(&item_id).await;
                return Ok(());
            }
            return Err("Failed to focus browser tab".to_string());
        }

        // Regular window: focus by handle
        if let Some(hwnd) = item.window_handle {
            focus_window_by_handle(hwnd);
            let _ = db::touch_item(&item_id).await;
            return Ok(());
        }
    }

    Err("Window not found".to_string())
}

/// Close a single window by item ID
#[tauri::command]
pub async fn close_window(item_id: String) -> Result<bool, String> {
    let items = db::get_all_tracked_items()
        .await
        .map_err(|e| e.to_string())?;

    if let Some(item) = items.iter().find(|i| i.id == item_id) {
        // Browser tab: close via CDP
        if item.item_type == ItemType::BrowserTab {
            let ok = cdp::close_cdp_tab(&item.id).await;
            // Delete from DB regardless — next scan will re-add if still open
            let _ = db::delete_tracked_item(&item_id).await;
            return Ok(ok);
        }

        // Regular window: close by handle
        if let Some(hwnd) = item.window_handle {
            close_window_by_handle(hwnd);
            // Always delete from DB — next scan will re-add if still open
            let _ = db::delete_tracked_item(&item_id).await;
            return Ok(true);
        }
    }

    Ok(false)
}

/// Find the window handle of a browser's main window by process name.
/// Used as a fallback when CDP activate fails.
fn find_browser_window_handle(process_name: &str) -> Option<i64> {
    let windows = crate::platform::enumerate_windows();
    windows
        .iter()
        .find(|w| w.process_name == process_name && w.window_handle.is_some())
        .and_then(|w| w.window_handle)
}

fn focus_window_by_handle(hwnd: i64) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::*;
        use windows::Win32::UI::WindowsAndMessaging::*;
        unsafe {
            let h = HWND(hwnd as *mut _);
            if !IsWindow(h).as_bool() {
                return;
            }
            if IsIconic(h).as_bool() {
                let _ = ShowWindow(h, SW_RESTORE);
            }
            let _ = SetForegroundWindow(h);
            let _ = BringWindowToTop(h);
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = hwnd;
    }
}

fn close_window_by_handle(hwnd: i64) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::*;
        use windows::Win32::UI::WindowsAndMessaging::*;
        unsafe {
            let h = HWND(hwnd as *mut _);
            if !IsWindow(h).as_bool() {
                return;
            }
            // Simulate clicking the X button
            let _ = SendMessageW(h, WM_SYSCOMMAND, WPARAM(0xF060), LPARAM(0));
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = hwnd;
    }
}

/// Launch a browser with remote debugging enabled so CDP can detect its tabs.
/// Tries Edge first (usually available on Windows), then Chrome.
/// Returns the browser name that was launched, or an error if neither is found.
#[tauri::command]
pub async fn launch_browser_debug() -> Result<String, String> {
    let port = 9222;

    // Try Edge first (built into Windows)
    let edge_paths = [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    ];
    for path in &edge_paths {
        if std::path::Path::new(path).exists() {
            std::process::Command::new(path)
                .arg(format!("--remote-debugging-port={}", port))
                .spawn()
                .map_err(|e| format!("Failed to launch Edge: {}", e))?;
            // Wait a moment for the browser to start and open the debug port
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            return Ok("Microsoft Edge".to_string());
        }
    }

    // Try Chrome
    let chrome_paths = [
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    ];
    for path in &chrome_paths {
        if std::path::Path::new(path).exists() {
            std::process::Command::new(path)
                .arg(format!("--remote-debugging-port={}", port))
                .spawn()
                .map_err(|e| format!("Failed to launch Chrome: {}", e))?;
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            return Ok("Google Chrome".to_string());
        }
    }

    Err("未找到 Edge 或 Chrome，请手动启动浏览器并添加 --remote-debugging-port=9222 参数".to_string())
}