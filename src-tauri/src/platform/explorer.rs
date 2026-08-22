//! File Explorer enumeration with real paths and per-tab granularity.
//!
//! Windows 11's File Explorer hosts multiple tabs inside one top-level
//! window, which EnumWindows cannot see. Every tab (and every classic
//! Explorer window) however registers itself as an entry in the Shell's
//! IShellWindows COM collection with its own IWebBrowser2 — LocationURL
//! gives the real folder path. Tabs of one window share a single HWND.
//!
//! Limitations (documented trade-offs):
//! - Two tabs showing the same folder in the SAME window collapse into one
//!   item (the collection exposes no stable per-tab id).
//! - Virtual folders (This PC, Recycle Bin) have no file:// URL; they are
//!   kept with path = None and don't participate in duplicate detection.
//! - Closing a single tab is only safe when its window has no sibling tabs
//!   (closing the shared HWND would kill them all) — see
//!   [`can_close_explorer_window`].

use super::{ItemType, TrackedItem};
use chrono::Utc;
use windows::core::{GUID, Interface, VARIANT};
use windows::Win32::Foundation::{HWND, SHANDLE_PTR};
use windows::Win32::System::Com::*;
use windows::Win32::UI::Shell::{IShellWindows, IWebBrowser2};
use windows::Win32::UI::WindowsAndMessaging::GetClassNameW;

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

        // Identity per (window, folder). Stable across scans so DB upserts
        // and task assignments (resource_key = path) survive re-scans.
        let identity = path
            .as_deref()
            .unwrap_or(&title)
            .to_lowercase()
            .replace('\\', "/");

        items.push(TrackedItem {
            id: format!("explorer_{}_{}", hwnd, identity),
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
