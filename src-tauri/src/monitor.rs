use crate::platform::{self, TrackedItem, ItemType};
use crate::db;
use crate::cdp;
use tokio::sync::Mutex;

/// Serializes scans: two overlapping get_tracked_items calls (manual refresh
/// racing a programmatic one) must not interleave their DB sync.
static SCAN_MUTEX: Mutex<()> = Mutex::const_new(());

/// Get current tracked items from DB (scans live windows + CDP tabs, cleans stale)
#[tauri::command]
pub async fn get_tracked_items() -> Result<Vec<TrackedItem>, String> {
    let _guard = SCAN_MUTEX.lock().await;

    // 1. Scan browser tabs via CDP (real URLs, all tabs)
    let cdp_tabs = cdp::fetch_browser_tabs().await;

    // 2. Scan windows via EnumWindows (file explorer, apps — browser main windows excluded)
    let mut window_items = platform::enumerate_windows();

    // 3. Explorer tabs via the Shell COM collection (real paths, Win11
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

    // 4. Merge: CDP tabs (if available) + explorer tabs + non-browser windows
    let mut all_items: Vec<TrackedItem> = Vec::new();
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
