use super::{ItemType, TrackedItem};
use chrono::Utc;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::System::Threading::*;

/// Enumerate all top-level windows on Windows
pub fn enumerate_windows() -> Vec<TrackedItem> {
    let mut items = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(enum_window_callback),
            LPARAM(&mut items as *mut Vec<TrackedItem> as isize),
        );
    }
    items
}

/// Find the handle of the first visible top-level window belonging to the
/// given process image name (e.g. "msedge.exe"). Used to focus a browser's
/// main window when CDP activation fails.
pub fn find_window_handle_by_process(image_name: &str) -> Option<i64> {
    struct FindState {
        target: String,
        found: Option<i64>,
    }

    unsafe extern "system" fn find_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = &mut *(lparam.0 as *mut FindState);
        if !IsWindowVisible(hwnd).as_bool() {
            return TRUE;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == std::process::id() {
            return TRUE;
        }
        if let Some(name) = process_image_name(pid) {
            if name.to_lowercase() == state.target && state.found.is_none() {
                state.found = Some(hwnd.0 as i64);
                return FALSE; // stop enumeration
            }
        }
        TRUE
    }

    let mut state = FindState {
        target: image_name.to_lowercase(),
        found: None,
    };
    unsafe {
        let _ = EnumWindows(
            Some(find_callback),
            LPARAM(&mut state as *mut FindState as isize),
        );
    }
    state.found
}

/// Real process image name (e.g. "chrome.exe") for a PID.
fn process_image_name(pid: u32) -> Option<String> {
    unsafe {
        if pid == 0 {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok =
            QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(process);
        if ok.is_ok() && len > 0 {
            let full = String::from_utf16_lossy(&buf[..len as usize]);
            full.rsplit(['\\', '/']).next().map(|s| s.to_string())
        } else {
            None
        }
    }
}

/// Undecorated windows lose the WS_SYSMENU style, which the Windows taskbar
/// relies on for the thumbnail hover preview's right-click menu and for
/// click-to-restore. Re-adding the style is invisible on an undecorated
/// window but restores those interactions (plus the Alt+Space menu).
pub fn restore_system_menu(hwnd: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongW, SetWindowLongW, SetWindowPos, GWL_STYLE, SWP_FRAMECHANGED, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_SYSMENU,
    };

    unsafe {
        let hwnd = HWND(hwnd as _);
        let style = GetWindowLongW(hwnd, GWL_STYLE);
        if style & WS_SYSMENU.0 as i32 == 0 {
            SetWindowLongW(hwnd, GWL_STYLE, style | WS_SYSMENU.0 as i32);
            // Let the shell re-read the window styles.
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
    }
}

unsafe extern "system" fn enum_window_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let items = &mut *(lparam.0 as *mut Vec<TrackedItem>);

    // 1. Must be visible
    if !IsWindowVisible(hwnd).as_bool() {
        return TRUE;
    }

    // 2. Skip our own process's windows (reliable regardless of title)
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == std::process::id() {
        return TRUE;
    }

    // 3. Skip windows with no title
    let mut title_buf = [0u16; 512];
    let title_len = GetWindowTextW(hwnd, &mut title_buf);
    if title_len == 0 {
        return TRUE;
    }
    let title = String::from_utf16_lossy(&title_buf[..title_len as usize]);
    let title = title.trim().to_string();
    if title.is_empty() {
        return TRUE;
    }

    // 4. Get window class
    let mut class_buf = [0u16; 256];
    let class_len = GetClassNameW(hwnd, &mut class_buf);
    let class_name = String::from_utf16_lossy(&class_buf[..class_len as usize]);

    // 5. Skip tiny windows (tooltips, icons, etc.)
    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_ok() {
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        if width <= 0 || height <= 0 {
            return TRUE;
        }
    }

    // 6. Skip known noise / system windows
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

    // 7. Real process name from the PID (falls back to the window class)
    let process_name = process_image_name(pid).unwrap_or_else(|| format!("app:{}", class_name));

    // Skip known background noise processes
    let skip_processes = [
        "nvidia",
        "nvcontainer",
        "cef-osc",
        "pcm_h5",
    ];
    if skip_processes
        .iter()
        .any(|p| process_name.to_lowercase().contains(p))
    {
        return TRUE;
    }

    // 8. Classify window type
    let (item_type, browser_name, url) = classify_window(&title, &process_name);

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
        icon: None,
        note: None,
                task_ids: Vec::new(),
    });

    TRUE
}

fn classify_window(
    title: &str,
    process_name: &str,
) -> (ItemType, Option<String>, Option<String>) {
    let process_lower = process_name.to_lowercase();

    // Known browsers (real image names)
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
