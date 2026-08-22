use crate::cdp;
use crate::db;
use crate::platform;
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
            // CDP focus failed — fall back to focusing the browser's main
            // window (looked up by process name, not via enumerate_windows,
            // which intentionally skips browser main windows).
            if let Some(hwnd) = platform::find_window_handle_by_process(&item.process_name) {
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
            let closed = close_window_by_handle(hwnd);
            // Always delete from DB — next scan will re-add if still open
            let _ = db::delete_tracked_item(&item_id).await;
            return Ok(closed);
        }
    }

    Ok(false)
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

fn close_window_by_handle(hwnd: i64) -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::*;
        use windows::Win32::UI::WindowsAndMessaging::*;
        unsafe {
            let h = HWND(hwnd as *mut _);
            if !IsWindow(h).as_bool() {
                return true; // already gone
            }
            // Simulate clicking the X button. PostMessage returns immediately
            // even if the target window is hung (SendMessage would block).
            PostMessageW(h, WM_SYSCOMMAND, WPARAM(0xF060), LPARAM(0)).is_ok() // SC_CLOSE = 0xF060
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = hwnd;
        false
    }
}

/// Launch a browser with remote debugging enabled so CDP can detect its tabs.
/// Tries Edge first (usually available on Windows), then Chrome.
/// Returns the browser name that was launched, or an error if none could be
/// started with a working debug port.
#[tauri::command]
pub async fn launch_browser_debug() -> Result<String, String> {
    let port = 9222;

    if cdp::is_debug_port_open(port).await {
        return Ok("调试模式已开启".to_string());
    }

    let mut candidates: Vec<(std::path::PathBuf, &str)> = Vec::new();

    // Edge (built into Windows)
    for p in [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    ] {
        candidates.push((std::path::PathBuf::from(p), "Microsoft Edge"));
    }

    // Chrome: machine-wide and per-user installs
    let mut chrome_paths = vec![
        r"C:\Program Files\Google\Chrome\Application\chrome.exe".to_string(),
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe".to_string(),
    ];
    if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
        chrome_paths.push(format!(
            r"{}\Google\Chrome\Application\chrome.exe",
            localappdata
        ));
    }
    for p in chrome_paths {
        candidates.push((std::path::PathBuf::from(p), "Google Chrome"));
    }

    let mut last_browser = String::new();
    for (path, name) in &candidates {
        if !path.exists() {
            continue;
        }
        last_browser = name.to_string();
        std::process::Command::new(path)
            .arg(format!("--remote-debugging-port={}", port))
            .spawn()
            .map_err(|e| format!("Failed to launch {}: {}", name, e))?;

        // Wait for the debug port to come up. Note: when a browser instance is
        // already running, the new process just forwards to it and the flag is
        // ignored — the port never opens, so we try the next browser.
        for _ in 0..8 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if cdp::is_debug_port_open(port).await {
                return Ok(name.to_string());
            }
        }
    }

    if !last_browser.is_empty() {
        return Err(format!(
            "已启动 {} 但调试端口未开启：浏览器可能已在运行，请完全退出浏览器后重试",
            last_browser
        ));
    }

    Err("未找到 Edge 或 Chrome，请手动启动浏览器并添加 --remote-debugging-port=9222 参数".to_string())
}
