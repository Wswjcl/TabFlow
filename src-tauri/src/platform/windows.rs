use super::{ItemType, TrackedItem};
use chrono::Utc;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Enumerate all top-level windows on Windows
pub fn enumerate_windows() -> Vec<TrackedItem> {
    let mut items = Vec::new();
    let mut total = 0usize;
    let mut skipped = 0usize;

    unsafe {
        let _ = EnumWindows(
            Some(enum_window_callback),
            LPARAM(&mut items as *mut Vec<TrackedItem> as isize),
        );
    }

    // Filter out our own app's windows
    let before = items.len();
    items.retain(|item: &TrackedItem| {
        !item.title.contains("TabFlow")
    });
    skipped += before - items.len();

    items
}

unsafe extern "system" fn enum_window_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let items = &mut *(lparam.0 as *mut Vec<TrackedItem>);

    // 1. Must be visible
    if !IsWindowVisible(hwnd).as_bool() {
        return TRUE;
    }

    // 2. Skip windows with no title
    let mut title_buf = [0u16; 512];
    let title_len = GetWindowTextW(hwnd, &mut title_buf);
    if title_len == 0 {
        return TRUE;
    }
    let title = String::from_utf16_lossy(&title_buf[..title_len as usize]);
    let title = title.trim().to_string();
    if title.is_empty() || title.len() < 1 {
        return TRUE;
    }

    // 4. Get window class
    let mut class_buf = [0u16; 256];
    let class_len = GetClassNameW(hwnd, &mut class_buf);
    let class_name = String::from_utf16_lossy(&class_buf[..class_len as usize]);

    // 5. Skip our own window
    if title == "TabFlow" || class_name.contains("tabflow") {
        return TRUE;
    }

    // 6. Skip tiny windows (tooltips, icons, etc.)
    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_ok() {
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        // Only skip extremely small or invisible windows
        if width <= 0 || height <= 0 {
            return TRUE;
        }
    }

    // 7. Skip known noise / system windows
    let skip_classes = [
        "Windows.UI.Core.CoreWindow",
        "ApplicationFrameWindow",
        "Shell_TrayWnd",
        "Progman",
        "WorkerW",
        "TaskSwitcherWnd",
        "SysListView32",
        "Button",
        "Static",
        "ToolbarWindow32",
        "msctls_statusbar32",
        "#32770",
        "CEF-OSC-WIDGET",           // NVIDIA overlay
        "pcm_h5_msg",               // Some messaging window
        "GDI+ Hook Window",
        "Dwm",
        "Msgr",
        "IME",
        "CiceroUIWndFrame",
        "OfficeTooltip",
        "tooltips_class32",
        "SysShadow",
    ];

    if skip_classes.contains(&class_name.as_str()) {
        return TRUE;
    }

    // 9. Determine process name & classify
    let process_name = infer_process_name(&title, &class_name);

    // Skip known background processes (very specific)
    let skip_processes = [
        "nvidia",
        "nvcontainer",
        "cef-osc",
        "pcm_h5",
    ];
    if skip_processes.iter().any(|p| process_name.to_lowercase().contains(p)) {
        return TRUE;
    }

    // 10. Classify window type
    let (item_type, browser_name, url) = classify_window(&title, &process_name, &class_name);

    // Skip browser main windows — their individual tabs are captured via CDP.
    // Showing the main window here would create duplicates (same title appears
    // both as an EnumWindows entry and as a CDP tab).
    if item_type == ItemType::BrowserTab {
        return TRUE;
    }

    let (item_type, path) = if class_name == "CabinetWClass" || class_name == "ExploreWClass" {
        (ItemType::ExplorerWindow, Some(title.clone()))
    } else {
        (item_type, None)
    };

    // Use HWND as stable ID instead of random UUID
    let hwnd_id = hwnd.0 as i64;

    items.push(TrackedItem {
        id: format!("hwnd_{}", hwnd_id),
        title,
        url,
        path,
        process_name,
        window_handle: Some(hwnd_id),
        item_type,
        browser_name,
        last_active_at: Utc::now().to_rfc3339(),
    });

    TRUE
}

fn infer_process_name(title: &str, class_name: &str) -> String {
    let title_lower = title.to_lowercase();
    let class_lower = class_name.to_lowercase();

    // Normalize title for matching (remove invisible chars)
    let title_normalized: String = title_lower
        .chars()
        .filter(|c| !c.is_control() || *c == ' ' || *c == '\n')
        .collect();

    if class_lower.contains("chrome_widgetwin") {
        // Check for Edge first (it's also a Chromium widget)
        if title_normalized.contains("microsoft edge") || title_normalized.contains("microsoftedge") {
            return "msedge.exe".to_string();
        }
        if title_normalized.contains("google chrome") || title_normalized.ends_with("google chrome") {
            return "chrome.exe".to_string();
        }
        return "chromium_host".to_string();
    }

    if class_lower.contains("mozilla") || title_lower.contains("mozilla firefox") {
        return "firefox.exe".to_string();
    }

    if class_name == "CabinetWClass" || class_name == "ExploreWClass" {
        return "explorer.exe".to_string();
    }

    // Known app classes
    if class_lower.contains("vscode") || class_lower.contains("code") {
        return "code.exe".to_string();
    }
    if class_lower.contains("notepad") {
        return "notepad.exe".to_string();
    }
    if class_lower.contains("sunawin") || class_lower.contains("sunaframe") {
        return "sunlogin.exe".to_string();
    }
    if class_lower.contains("wechat") || class_lower.contains("wechatmainwnd") {
        return "wechat.exe".to_string();
    }
    if class_lower.contains("qq") || title_lower.contains("qq") {
        return "qq.exe".to_string();
    }
    if class_lower.contains("afx:") || class_lower.contains("afxframe") {
        return "mfc_app.exe".to_string();
    }

    format!("app:{}", class_name)
}

fn classify_window(
    title: &str,
    process_name: &str,
    _class_name: &str,
) -> (ItemType, Option<String>, Option<String>) {
    let process_lower = process_name.to_lowercase();

    // Known browsers
    if process_lower == "chrome.exe" {
        let url = extract_url_from_title(title, "chrome");
        return (ItemType::BrowserTab, Some("chrome".into()), url);
    }
    if process_lower == "msedge.exe" {
        let url = extract_url_from_title(title, "edge");
        return (ItemType::BrowserTab, Some("edge".into()), url);
    }
    if process_lower == "firefox.exe" {
        let url = extract_url_from_title(title, "firefox");
        return (ItemType::BrowserTab, Some("firefox".into()), url);
    }

    (ItemType::AppWindow, None, None)
}

fn extract_url_from_title(title: &str, browser: &str) -> Option<String> {
    // Normalize: remove zero-width / invisible chars
    let clean_title: String = title
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\r' || *c == '\t')
        .collect();
    let clean_title = clean_title
        .replace('\u{200B}', "")
        .replace('\u{200C}', "")
        .replace('\u{200D}', "")
        .replace('\u{FEFF}', "");

    // Suffix patterns per browser
    let suffixes: &[&str] = match browser {
        "chrome" => &[" - Google Chrome"],
        "edge" => &[" - Microsoft Edge", " - Microsoft\u{200B}Edge"],
        "firefox" => &[" — Mozilla Firefox"],
        _ => return None,
    };

    for suffix in suffixes {
        if clean_title.ends_with(suffix) {
            let page_title = clean_title
                .strip_suffix(suffix)
                .unwrap_or(&clean_title)
                .trim()
                .to_string();
            if !page_title.is_empty() {
                // Use the full page title (including sub-parts like "Foo - Bar")
                return Some(format!("page:{}", page_title.to_lowercase()));
            }
        }
    }

    // No suffix matched → can't reliably extract page title
    None
}