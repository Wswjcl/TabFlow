//! File Explorer enumeration with real paths and per-tab granularity.
//!
//! Windows 11's File Explorer hosts multiple tabs inside one top-level
//! window, which EnumWindows cannot see. Every tab (and every classic
//! Explorer window) however registers itself as an entry in the Shell's
//! IShellWindows COM collection with its own IWebBrowser2 — LocationURL
//! gives the real folder path. Tabs of one window share a single HWND.
//!
//! Limitations (documented trade-offs):
//! - Virtual folders (This PC, Recycle Bin) have no file:// URL; they are
//!   kept with path = None and don't participate in duplicate detection.
//! - Closing tabs: single-tab windows close via the window handle; tabs in
//!   multi-tab windows close via UI Automation (select the TabItem, then
//!   Ctrl+W) — see [`close_explorer_tab`]. If the tab strip isn't reachable
//!   over UIA the close is skipped rather than risking the whole window.

use super::{ItemType, TrackedItem};
use chrono::Utc;
use std::collections::HashMap;
use windows::core::{GUID, Interface, VARIANT};
use windows::Win32::Foundation::{HWND, LPARAM, SHANDLE_PTR, WPARAM};
use windows::Win32::System::Com::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_CONTROL;
use windows::Win32::UI::Shell::{IShellWindows, IWebBrowser2};
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, PostMessageW, WM_KEYDOWN, WM_KEYUP};

/// CLSID of the ShellWindows coclass (not exposed by the windows crate).
const CLSID_SHELL_WINDOWS: GUID = GUID::from_u128(0x9BA05972_F6A8_11CF_A442_00A0C90A8F39);

/// Enumerate all File Explorer tabs/windows.
/// Ok(..) means the COM collection was read successfully (possibly empty);
/// Err means COM failed and callers should fall back to the EnumWindows
/// approximation.
pub fn enumerate_explorer_items() -> Result<Vec<TrackedItem>, ()> {
    // COM needs an initialized thread; use a dedicated STA thread so this is
    // safe to call from any (including async) context.
    std::thread::spawn(|| {
        unsafe {
            if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
                return Err(());
            }
        }
        let result = unsafe { enumerate_via_shell_windows() };
        unsafe { CoUninitialize() };
        result
    })
    .join()
    .map_err(|_| ())?
}

unsafe fn enumerate_via_shell_windows() -> Result<Vec<TrackedItem>, ()> {
    let shell_windows: IShellWindows =
        CoCreateInstance(&CLSID_SHELL_WINDOWS, None, CLSCTX_ALL).map_err(|_| ())?;

    let count = shell_windows.Count().map_err(|_| ())?;
    let now = Utc::now().to_rfc3339();
    let mut items = Vec::new();
    // Occurrence counter per (window, folder): IShellWindows exposes no
    // stable per-tab id, so the 2nd+ tab showing the same folder in the
    // same window gets a "_n" suffix. The occurrences are interchangeable
    // (same path/title/hwnd); when one closes, the highest suffix simply
    // disappears on the next scan.
    let mut occurrences: HashMap<(i64, String), usize> = HashMap::new();

    for i in 0..count {
        let variant = VARIANT::from(i);
        let dispatch = match shell_windows.Item(&variant) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let browser: IWebBrowser2 = match dispatch.cast() {
            Ok(b) => b,
            Err(_) => continue,
        };

        let hwnd: i64 = match browser.HWND() {
            Ok(SHANDLE_PTR(h)) => h as i64,
            Err(_) => continue,
        };
        // The collection also contains the desktop and other shell views;
        // keep only real Explorer windows.
        if !is_explorer_window(hwnd) {
            continue;
        }

        let title = browser
            .LocationName()
            .map(|b| b.to_string())
            .unwrap_or_default();
        let url = browser
            .LocationURL()
            .map(|b| b.to_string())
            .unwrap_or_default();
        let path = file_url_to_path(&url);

        if title.is_empty() && path.is_none() {
            continue;
        }

        // Identity per (window, folder), with an occurrence suffix for
        // same-folder tabs in the same window. Stable across scans so DB
        // upserts and task assignments (resource_key = path) survive
        // re-scans, and two tabs with the same folder stay two items.
        let identity = path
            .as_deref()
            .unwrap_or(&title)
            .to_lowercase()
            .replace('\\', "/");
        let occurrence = occurrences
            .entry((hwnd, identity.clone()))
            .or_insert(0);
        let id = explorer_item_id(hwnd, &identity, *occurrence);
        *occurrence += 1;

        items.push(TrackedItem {
            id,
            title: if title.is_empty() {
                path.clone().unwrap_or_default()
            } else {
                title
            },
            url: None,
            path,
            process_name: "explorer.exe".to_string(),
            window_handle: Some(hwnd),
            item_type: ItemType::ExplorerWindow,
            browser_name: None,
            last_active_at: now.clone(),
        });
    }

    // Deterministic order (duplicate grouping keeps first-item semantics)
    items.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(items)
}

/// Item id for an Explorer tab. Occurrence 0 keeps the plain form (stable
/// for the common single-tab case and backward compatible with existing
/// rows); 2nd+ tabs of the same folder in the same window get "_n".
fn explorer_item_id(hwnd: i64, identity: &str, occurrence: usize) -> String {
    if occurrence == 0 {
        format!("explorer_{}_{}", hwnd, identity)
    } else {
        format!("explorer_{}_{}_{}", hwnd, identity, occurrence)
    }
}

/// True when `hwnd` is a classic Explorer window (not the desktop or IE).
fn is_explorer_window(hwnd: i64) -> bool {
    unsafe {
        let h = HWND(hwnd as *mut _);
        let mut class_buf = [0u16; 256];
        let len = GetClassNameW(h, &mut class_buf);
        let class_name = String::from_utf16_lossy(&class_buf[..len as usize]);
        class_name == "CabinetWClass" || class_name == "ExploreWClass"
    }
}

/// Closing an Explorer tab means closing its window handle — which would kill
/// sibling tabs of the same window. Only allow it when this is the only tab.
pub fn can_close_explorer_window(hwnd: i64, items: &[TrackedItem]) -> bool {
    items
        .iter()
        .filter(|i| i.item_type == ItemType::ExplorerWindow && i.window_handle == Some(hwnd))
        .count()
        <= 1
}

/// Close a single Explorer tab in a multi-tab window via UI Automation:
/// select the TabItem whose name matches `tab_title`, then post Ctrl+W
/// (closes exactly the active tab). Falls back to false when the tab strip
/// isn't reachable — callers should then leave the tab alone.
///
/// Same-folder tabs are interchangeable, so closing "a tab named X" is
/// equivalent to closing any specific one of them.
pub fn close_explorer_tab(hwnd: i64, tab_title: &str) -> bool {
    let title = tab_title.to_string();
    std::thread::spawn(move || {
        unsafe {
            if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
                return false;
            }
        }
        let ok = unsafe { uia_close_tab(hwnd, &title) };
        unsafe { CoUninitialize() };
        ok
    })
    .join()
    .unwrap_or(false)
}

unsafe fn uia_close_tab(hwnd: i64, tab_title: &str) -> bool {
    let automation: IUIAutomation =
        match CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) {
            Ok(a) => a,
            Err(_) => return false,
        };
    let root = match automation.ElementFromHandle(HWND(hwnd as *mut _)) {
        Ok(e) => e,
        Err(_) => return false,
    };

    // TabItem with matching display name in this window's tab strip
    let name_cond = automation.CreatePropertyCondition(
        UIA_NamePropertyId,
        &VARIANT::from(tab_title),
    );
    let type_cond = automation.CreatePropertyCondition(
        UIA_ControlTypePropertyId,
        &VARIANT::from(UIA_TabItemControlTypeId.0 as i32),
    );
    let (name_cond, type_cond) = match (name_cond, type_cond) {
        (Ok(n), Ok(t)) => (n, t),
        _ => return false,
    };
    let cond = match automation.CreateAndCondition(&name_cond, &type_cond) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let tab = match root.FindFirst(TreeScope_Subtree, &cond) {
        Ok(t) => t, // a null/no-match result fails below on pattern lookup
        Err(_) => return false,
    };

    // Activate the tab…
    let selection: IUIAutomationSelectionItemPattern =
        match tab.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId)
        {
            Ok(p) => p,
            Err(_) => return false,
        };
    if selection.Select().is_err() {
        return false;
    }
    // …give the activation a moment to land before closing the active tab
    std::thread::sleep(std::time::Duration::from_millis(150));
    post_ctrl_w(hwnd);
    true
}

fn post_ctrl_w(hwnd: i64) {
    unsafe {
        let h = HWND(hwnd as *mut _);
        let vk_ctrl = VK_CONTROL.0 as usize;
        let vk_w = 'W' as usize;
        let _ = PostMessageW(h, WM_KEYDOWN, WPARAM(vk_ctrl), LPARAM(0));
        let _ = PostMessageW(h, WM_KEYDOWN, WPARAM(vk_w), LPARAM(0));
        let _ = PostMessageW(h, WM_KEYUP, WPARAM(vk_w), LPARAM(0));
        let _ = PostMessageW(h, WM_KEYUP, WPARAM(vk_ctrl), LPARAM(0));
    }
}

/// Convert a `file:///C:/foo%20bar` LocationURL into a normal Windows path.
fn file_url_to_path(url: &str) -> Option<String> {
    let rest = url.strip_prefix("file:///")?;
    let decoded = percent_decode(rest.as_bytes())?;
    let s = String::from_utf8(decoded).ok()?;
    if s.is_empty() {
        return None;
    }
    Some(s.replace('/', "\\"))
}

/// Minimal percent-decoder for file URLs (UTF-8 bytes).
fn percent_decode(input: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'%' {
            // get() returns None on out-of-bounds (dangling % sequence)
            let hi = hex_val(*input.get(i + 1)?)?;
            let lo = hex_val(*input.get(i + 2)?)?;
            out.push(hi * 16 + lo);
            i += 3;
        } else {
            out.push(input[i]);
            i += 1;
        }
    }
    Some(out)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_file_url_to_windows_path() {
        assert_eq!(
            file_url_to_path("file:///C:/Users/foo/My%20Docs"),
            Some(r"C:\Users\foo\My Docs".to_string())
        );
        assert_eq!(
            file_url_to_path("file:///D:/data"),
            Some(r"D:\data".to_string())
        );
    }

    #[test]
    fn rejects_non_file_urls() {
        assert_eq!(file_url_to_path("https://example.com"), None);
        assert_eq!(file_url_to_path(""), None);
        assert_eq!(file_url_to_path("file:///"), None);
    }

    #[test]
    fn percent_decode_handles_truncated_sequences() {
        assert_eq!(percent_decode(b"100%"), None); // dangling %
        assert_eq!(percent_decode(b"%zz"), None); // invalid hex
        assert_eq!(percent_decode(b"plain"), Some(b"plain".to_vec()));
        assert_eq!(percent_decode(b"a%20b"), Some(b"a b".to_vec()));
    }

    #[test]
    fn same_folder_tabs_get_distinct_ids() {
        // 1st occurrence: plain form (stable, backward compatible)
        assert_eq!(
            explorer_item_id(100, "c:/users/foo", 0),
            "explorer_100_c:/users/foo"
        );
        // 2nd/3rd tab of the same folder in the same window: suffixed
        assert_eq!(
            explorer_item_id(100, "c:/users/foo", 1),
            "explorer_100_c:/users/foo_1"
        );
        assert_eq!(
            explorer_item_id(100, "c:/users/foo", 2),
            "explorer_100_c:/users/foo_2"
        );
        // Same folder in a different window: different hwnd → distinct anyway
        assert_ne!(
            explorer_item_id(100, "c:/users/foo", 0),
            explorer_item_id(200, "c:/users/foo", 0)
        );
    }

    /// Manual smoke test against the live system:
    /// `cargo test --lib live_enumeration -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_enumeration() {
        match super::enumerate_explorer_items() {
            Ok(items) => {
                println!("explorer items: {}", items.len());
                for i in items {
                    println!(
                        "  [{}] title={:?} path={:?} hwnd={}",
                        i.id,
                        i.title,
                        i.path.as_deref().unwrap_or(""),
                        i.window_handle.unwrap_or(0)
                    );
                }
            }
            Err(_) => println!("COM enumeration failed"),
        }
    }
}
