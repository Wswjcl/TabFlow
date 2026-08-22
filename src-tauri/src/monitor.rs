use crate::platform::{self, TrackedItem, ItemType};
use crate::db;
use crate::cdp;
use crate::browser;
use std::collections::HashSet;
use tokio::sync::Mutex;

/// Serializes scans: two overlapping get_tracked_items calls (manual refresh
/// racing a programmatic one) must not interleave their DB sync.
static SCAN_MUTEX: Mutex<()> = Mutex::const_new(());

/// Get current tracked items from DB (scans live windows + CDP tabs, cleans stale)
#[tauri::command]
pub async fn get_tracked_items() -> Result<Vec<TrackedItem>, String> {
    let _guard = SCAN_MUTEX.lock().await;

    // 1. Browser tabs from connected extensions (works on normally-running
    //    browsers) — they take priority over CDP for the same browser so the
    //    same tab never appears twice.
    let ext_tabs = browser::get_extension_tabs().await;
    let ext_browsers: HashSet<String> = browser::connected_browsers().await.into_iter().collect();

    // 2. CDP tabs (requires debug mode) for browsers WITHOUT an extension
    let cdp_tabs: Vec<TrackedItem> = cdp::fetch_browser_tabs()
        .await
        .into_iter()
        .filter(|t| {
            t.browser_name
                .as_deref()
                .map(|b| !ext_browsers.contains(b))
                .unwrap_or(true)
        })
        .collect();

    // 3. Scan windows via EnumWindows (file explorer, apps — browser main windows excluded)
    let mut window_items = platform::enumerate_windows();

    // 4. Explorer tabs via the Shell COM collection (real paths, Win11
    //    per-tab). When COM succeeds it supersedes the EnumWindows
    //    approximation (title-as-path, one entry per window); on failure we
    //    keep the approximation as fallback.
    let mut explorer_tabs: Vec<TrackedItem> = Vec::new();
    match platform::enumerate_explorer_items() {
        Ok(tabs) => {
            explorer_tabs = tabs;
            window_items.retain(|it| it.item_type != ItemType::ExplorerWindow);
        }
        Err(e) => eprintln!("Explorer COM enumeration failed, falling back to EnumWindows: {:?}", e),
    }

    // 5. Merge: extension tabs + CDP tabs (if any) + explorer tabs + windows
    let mut all_items: Vec<TrackedItem> = Vec::new();
    all_items.extend(ext_tabs);
    let has_cdp = !cdp_tabs.is_empty();

    if has_cdp {
        // CDP is working → use its tabs + non-browser EnumWindows
        all_items.extend(cdp_tabs);
        // EnumWindows already skips BrowserTab items, but double-check
        for item in window_items {
            if item.item_type != ItemType::BrowserTab {
                all_items.push(item);
            }
        }
    } else {
        // CDP not available → fall back to EnumWindows for everything.
        // Promote browser windows to AppWindow so they show up with a usable
        // window_handle (can still be focused/closed via HWND).
        for item in &mut window_items {
            if item.item_type == ItemType::BrowserTab {
                item.item_type = ItemType::AppWindow;
            }
        }
        all_items.extend(window_items);
    }

    all_items.extend(explorer_tabs);

    // 5. Atomically store & clean up stale rows
    if let Err(e) = db::sync_items(&all_items).await {
        eprintln!("Failed to sync items: {}", e);
    }

    // 6. Return from DB
    db::get_all_tracked_items()
        .await
        .map_err(|e| e.to_string())
}

/// Get all current windows (real-time snapshot, does not touch DB)
#[tauri::command]
pub fn get_all_windows() -> Vec<TrackedItem> {
    platform::enumerate_windows()
}
