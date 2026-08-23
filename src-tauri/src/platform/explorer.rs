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
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, SHANDLE_PTR, TRUE, WPARAM};
use windows::Win32::System::Com::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{MapVirtualKeyW, MAPVK_VK_TO_VSC, VK_CONTROL};
use windows::Win32::UI::Shell::{IShellWindows, IWebBrowser2};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, PostMessageW, SystemParametersInfoW, EnumChildWindows, WM_KEYDOWN, WM_KEYUP,
    OBJID_CLIENT, SPIF_UPDATEINIFILE, SPI_GETSCREENREADER, SPI_SETSCREENREADER,
};

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
            task_ids: Vec::new(),
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

/// Close a single Explorer tab in a multi-tab window via UI Automation.
/// Chain (first that works wins):
///   1. invoke the TabItem's own close button — most precise, no keyboard,
///      no focus changes, locale-independent
///   2. select the tab, then post Ctrl+W with proper scan codes
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

unsafe fn uia_automation() -> Option<IUIAutomation> {
    CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()
}

/// Nudge Explorer into building its full accessibility tree. Without an
/// assistive-technology client present, the Win11 content area (including
/// the tab strip) is simply not exposed over UIA — the tree only contains
/// the title bar.
///
/// 1. An OBJID_CLIENT request (WM_GETOBJECT) triggers provider creation in
///    Chromium-family UI frameworks.
/// 2. The system "screen reader" flag makes apps keep the tree alive.
///    Returns true when WE flipped the flag (caller must restore it after).
unsafe fn activate_accessibility(hwnd: i64) -> bool {
    // OBJID_CLIENT request
    let mut acc: *mut core::ffi::c_void = std::ptr::null_mut();
    let riid = &<IAccessible as Interface>::IID;
    let _ = AccessibleObjectFromWindow(
        HWND(hwnd as *mut _),
        OBJID_CLIENT.0 as u32,
        riid,
        &mut acc as *mut *mut core::ffi::c_void,
    );
    if !acc.is_null() {
        // We own the returned reference — release it.
        let accessible: IAccessible = std::mem::transmute(acc);
        drop(accessible);
    }

    // Screen-reader flag (restore later if we changed it)
    let mut prev: u32 = 0;
    let _ = SystemParametersInfoW(
        SPI_GETSCREENREADER,
        0,
        Some(&mut prev as *mut u32 as *mut core::ffi::c_void),
        Default::default(),
    );
    if prev == 0 {
        let _ = SystemParametersInfoW(SPI_SETSCREENREADER, 1, None, SPIF_UPDATEINIFILE);
        true
    } else {
        false
    }
}

unsafe fn restore_screen_reader_flag() {
    let _ = SystemParametersInfoW(SPI_SETSCREENREADER, 0, None, SPIF_UPDATEINIFILE);
}

/// All descendant HWND ids of a window (the tab strip lives in XAML-island
/// child HWNDs that may not be bridged into the parent's UIA tree).
fn child_hwnds(hwnd: i64) -> Vec<i64> {
    struct Ctx {
        out: Vec<i64>,
    }
    unsafe extern "system" fn callback(h: HWND, l: LPARAM) -> BOOL {
        let ctx = &mut *(l.0 as *mut Ctx);
        ctx.out.push(h.0 as i64);
        TRUE
    }
    let mut ctx = Ctx { out: Vec::new() };
    unsafe {
        let _ = EnumChildWindows(HWND(hwnd as *mut _), Some(callback), LPARAM(&mut ctx as *mut _ as isize));
    }
    ctx.out
}

/// Find ALL TabItem elements of a window: searches the top-level element
/// plus every descendant HWND (XAML islands are separate HWNDs), after
/// activating the accessibility tree. The same tab can appear in several
/// subtree bridges — callers must treat the list as containing duplicates.
unsafe fn find_all_tab_items(
    automation: &IUIAutomation,
    hwnd: i64,
) -> Vec<IUIAutomationElement> {
    let Ok(type_cond) = automation.CreatePropertyCondition(
        UIA_ControlTypePropertyId,
        &VARIANT::from(UIA_TabItemControlTypeId.0 as i32),
    ) else {
        return Vec::new();
    };

    let mut result: Vec<IUIAutomationElement> = Vec::new();
    for h in std::iter::once(hwnd).chain(child_hwnds(hwnd)) {
        let Ok(root) = automation.ElementFromHandle(HWND(h as *mut _)) else {
            continue;
        };
        let Ok(found) = root.FindAll(TreeScope_Subtree, &type_cond) else {
            continue;
        };
        let Ok(len) = found.Length() else { continue };
        for i in 0..len {
            let Ok(el) = found.GetElement(i) else { continue };
            // drop elements that are bridges to an already-covered subtree
            let mut duplicate = false;
            for seen in &result {
                if automation
                    .CompareElements(&el, seen)
                    .map(|same| same.as_bool())
                    .unwrap_or(false)
                {
                    duplicate = true;
                    break;
                }
            }
            if !duplicate {
                result.push(el);
            }
        }
    }
    result
}

/// True when the element contains a Button child (real tabs carry their
/// close button; navigation-pane nodes matched as TabItem do not).
unsafe fn has_close_button(automation: &IUIAutomation, el: &IUIAutomationElement) -> bool {
    let Ok(cond) = automation.CreatePropertyCondition(
        UIA_ControlTypePropertyId,
        &VARIANT::from(UIA_ButtonControlTypeId.0 as i32),
    ) else {
        return false;
    };
    match el.FindFirst(TreeScope_Subtree, &cond) {
        Ok(btn) => !btn.as_raw().is_null(),
        Err(_) => false,
    }
}

/// All TabItem names of an Explorer window (tab strip order), for
/// diagnostics and tests — read-only, performs no actions.
#[allow(dead_code)] // exercised by the ignored live diagnostics tests
pub fn explorer_tab_names(hwnd: i64) -> Vec<String> {
    std::thread::spawn(move || {
        unsafe {
            if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
                return Vec::new();
            }
        }
        let names = unsafe {
            let Some(automation) = uia_automation() else {
                return Vec::new();
            };
            let flag_flipped = activate_accessibility(hwnd);
            // trees build asynchronously — give Explorer a moment
            std::thread::sleep(std::time::Duration::from_millis(300));
            let names = find_all_tab_items(&automation, hwnd)
                .iter()
                .filter_map(|el| el.CurrentName().ok().map(|n| n.to_string()))
                .collect();
            if flag_flipped {
                restore_screen_reader_flag();
            }
            names
        };
        unsafe { CoUninitialize() };
        names
    })
    .join()
    .unwrap_or_default()
}

/// Read-only dump of an Explorer window's UIA control tree (bounded depth),
/// for diagnostics — performs no actions.
#[allow(dead_code)] // exercised by the ignored live diagnostics tests
pub fn explorer_tree_dump(hwnd: i64) -> Vec<String> {
    std::thread::spawn(move || {
        unsafe {
            if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
                return vec!["CoInitializeEx failed".to_string()];
            }
        }
        let out = unsafe {
            let mut out = Vec::new();
            let Some(automation) = uia_automation() else {
                return vec!["CUIAutomation failed".to_string()];
            };
            let Ok(root) = automation.ElementFromHandle(HWND(hwnd as *mut _)) else {
                return vec!["ElementFromHandle failed".to_string()];
            };
            let walker = match automation.ControlViewWalker() {
                Ok(w) => w,
                Err(e) => return vec![format!("ControlViewWalker failed: {e}")],
            };
            dump_element(&walker, &root, 0, &mut out);
            out
        };
        unsafe { CoUninitialize() };
        out
    })
    .join()
    .unwrap_or_default()
}

unsafe fn dump_element(
    walker: &IUIAutomationTreeWalker,
    element: &IUIAutomationElement,
    depth: usize,
    out: &mut Vec<String>,
) {
    if depth > 6 || out.len() > 250 {
        return;
    }
    let name = element
        .CurrentName()
        .map(|b| b.to_string())
        .unwrap_or_default();
    let ctype = element.CurrentControlType().map(|t| t.0).unwrap_or(0);
    let class = element
        .CurrentClassName()
        .map(|b| b.to_string())
        .unwrap_or_default();
    out.push(format!(
        "{}ct={} class={:?} name={:?}",
        "  ".repeat(depth),
        ctype,
        class,
        name
    ));

    let mut child = walker.GetFirstChildElement(element).ok();
    while let Some(c) = child {
        dump_element(walker, &c, depth + 1, out);
        if out.len() > 250 {
            return;
        }
        child = walker.GetNextSiblingElement(&c).ok();
    }
}

unsafe fn find_tab_item(
    automation: &IUIAutomation,
    hwnd: i64,
    tab_title: &str,
) -> Option<IUIAutomationElement> {
    let mut fallback = None;
    for el in find_all_tab_items(automation, hwnd) {
        let name_matches = el
            .CurrentName()
            .map(|n| n.to_string() == tab_title)
            .unwrap_or(false);
        if !name_matches {
            continue;
        }
        // Real tabs carry a close button; navigation-pane look-alikes don't.
        if has_close_button(automation, &el) {
            return Some(el);
        }
        if fallback.is_none() {
            fallback = Some(el);
        }
    }
    fallback
}

/// Invoke the close Button inside a TabItem (Invoke pattern, then
/// LegacyIAccessible default action).
unsafe fn invoke_tab_close_button(
    automation: &IUIAutomation,
    tab: &IUIAutomationElement,
) -> bool {
    let Ok(cond) = automation.CreatePropertyCondition(
        UIA_ControlTypePropertyId,
        &VARIANT::from(UIA_ButtonControlTypeId.0 as i32),
    ) else {
        return false;
    };
    let Ok(button) = tab.FindFirst(TreeScope_Subtree, &cond) else {
        return false;
    };
    if let Ok(invoke) = button.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) {
        if invoke.Invoke().is_ok() {
            return true;
        }
    }
    if let Ok(legacy) = button
        .GetCurrentPatternAs::<IUIAutomationLegacyIAccessiblePattern>(UIA_LegacyIAccessiblePatternId)
    {
        if legacy.DoDefaultAction().is_ok() {
            return true;
        }
    }
    false
}

unsafe fn select_tab(tab: &IUIAutomationElement) -> bool {
    match tab.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId) {
        Ok(pattern) => pattern.Select().is_ok(),
        Err(_) => false,
    }
}

unsafe fn uia_close_tab(hwnd: i64, tab_title: &str) -> bool {
    let Some(automation) = uia_automation() else {
        return false;
    };
    let flag_flipped = activate_accessibility(hwnd);
    // trees build asynchronously — give Explorer a moment
    std::thread::sleep(std::time::Duration::from_millis(300));

    let result = (|| {
        let Some(tab) = find_tab_item(&automation, hwnd, tab_title) else {
            return false;
        };
        // Only act on real tabs (they carry a close button). A name match
        // without one is a navigation-pane look-alike — refuse rather than
        // risk Ctrl+W closing the wrong tab.
        if !has_close_button(&automation, &tab) {
            return false;
        }

        // 1) The tab's own close button
        if invoke_tab_close_button(&automation, &tab) {
            return true;
        }

        // 2) Activate the tab, then Ctrl+W (closes exactly the active tab)
        if select_tab(&tab) {
            // give the activation a moment to land
            std::thread::sleep(std::time::Duration::from_millis(150));
            post_ctrl_w(hwnd);
            return true;
        }

        false
    })();

    if flag_flipped {
        restore_screen_reader_flag();
    }
    result
}

fn post_ctrl_w(hwnd: i64) {
    unsafe {
        let h = HWND(hwnd as *mut _);
        let vk_ctrl = VK_CONTROL.0 as usize;
        let vk_w = 'W' as usize;
        // lParam: repeat=1 | scan code << 16 (| previous/up transition bits)
        let scan_ctrl = MapVirtualKeyW(vk_ctrl as u32, MAPVK_VK_TO_VSC) as isize;
        let scan_w = MapVirtualKeyW(vk_w as u32, MAPVK_VK_TO_VSC) as isize;
        let down = |scan: isize| LPARAM(1 | (scan << 16));
        let up =
            |scan: isize| LPARAM(1 | (scan << 16) | (1 << 30) | (1 << 31));
        let _ = PostMessageW(h, WM_KEYDOWN, WPARAM(vk_ctrl), down(scan_ctrl));
        let _ = PostMessageW(h, WM_KEYDOWN, WPARAM(vk_w), down(scan_w));
        let _ = PostMessageW(h, WM_KEYUP, WPARAM(vk_w), up(scan_w));
        let _ = PostMessageW(h, WM_KEYUP, WPARAM(vk_ctrl), up(scan_ctrl));
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

    /// Manual read-only diagnostic: shows each window's UIA TabItem names
    /// next to the COM LocationName — verifies the name matching that
    /// close_explorer_tab relies on. Performs no actions.
    /// `cargo test --lib live_tab_names -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_tab_names() {
        let items = super::enumerate_explorer_items().unwrap_or_default();
        let mut seen_hwnds = std::collections::HashSet::new();
        for i in items {
            let Some(hwnd) = i.window_handle else { continue };
            if !seen_hwnds.insert(hwnd) {
                continue;
            }
            let names = super::explorer_tab_names(hwnd);
            println!("hwnd={} com_title={:?}", hwnd, i.title);
            println!("  uia tab names: {:?}", names);
        }
    }

    /// Manual read-only diagnostic: dumps the UIA control tree of the first
    /// Explorer window. `cargo test --lib live_tree_dump -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_tree_dump() {
        let items = super::enumerate_explorer_items().unwrap_or_default();
        let Some(first) = items.iter().find_map(|i| i.window_handle) else {
            println!("no explorer windows open");
            return;
        };
        println!("hwnd={} tree:", first);
        for line in super::explorer_tree_dump(first) {
            println!("{}", line);
        }
    }

    /// Manual end-to-end test: closes OUR OWN throwaway window (open C:\
    /// first). Never touches other windows — it filters by the exact title.
    /// `cargo test --lib live_close_tab -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_close_tab() {
        let items = super::enumerate_explorer_items().unwrap_or_default();
        let Some(item) = items.iter().find(|i| i.title == "Windows-SSD (C:)").cloned() else {
            println!("打开一个 C:\\ 资源管理器窗口后重跑");
            return;
        };
        let Some(hwnd) = item.window_handle else { return };
        let closed = super::close_explorer_tab(hwnd, &item.title);
        println!("close_explorer_tab returned {}", closed);
        std::thread::sleep(std::time::Duration::from_millis(1200));
        let after = super::enumerate_explorer_items().unwrap_or_default();
        let gone = !after.iter().any(|i| i.window_handle == Some(hwnd));
        println!("window gone after close: {}", gone);
        assert!(closed && gone, "close chain did not close the window");
    }
}
