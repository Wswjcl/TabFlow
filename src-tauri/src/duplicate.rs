use crate::db;
use crate::platform::{DuplicateGroup, TrackedItem, ItemType};
use uuid::Uuid;
use std::collections::{HashMap, HashSet};

/// Detect duplicate windows/tabs from DB
#[tauri::command]
pub async fn detect_duplicates() -> Result<Vec<DuplicateGroup>, String> {
    db::detect_and_store_duplicates()
        .await
        .map_err(|e| e.to_string())?;

    let items = db::get_all_tracked_items().await.map_err(|e| e.to_string())?;
    Ok(find_duplicates(&items))
}

/// Stable identity of an item, independent of the window instance:
/// a URL / explorer path / process+title. Used for duplicate grouping and
/// for task assignments (which must survive the window closing).
pub fn resource_key(item: &TrackedItem) -> String {
    match &item.item_type {
        ItemType::BrowserTab => {
            if let Some(ref url) = item.url {
                if url.starts_with("http://") || url.starts_with("https://") {
                    format!("url:{}", normalize_url(url))
                } else {
                    // Pseudo-URL from the title fallback ("page:...")
                    format!("url:{}", url.trim_end_matches('/').to_lowercase())
                }
            } else {
                format!("browser_standalone:{}", item.id)
            }
        }
        ItemType::ExplorerWindow => item
            .path
            .as_ref()
            .map(|p| format!("path:{}", p.to_lowercase()))
            .unwrap_or_else(|| format!("explorer_standalone:{}", item.id)),
        ItemType::AppWindow => format!(
            "app:{}:{}",
            item.process_name.to_lowercase(),
            item.title.to_lowercase()
        ),
    }
}

/// Normalize a URL for duplicate comparison:
/// - treat http and https as equivalent, lowercase scheme+host
/// - drop the fragment (#...)
/// - strip known tracking query params (utm_*, gclid, ...) while keeping
///   meaningful ones (e.g. search queries) so different searches still differ
/// - trim trailing slashes
fn normalize_url(url: &str) -> String {
    let no_fragment = url.split('#').next().unwrap_or(url);
    let (base, query) = match no_fragment.split_once('?') {
        Some((b, q)) => (b, Some(q)),
        None => (no_fragment, None),
    };

    let mut base_lower = base.to_lowercase();
    if let Some(rest) = base_lower.strip_prefix("https://") {
        base_lower = format!("http://{}", rest);
    }
    let base_trimmed = base_lower.trim_end_matches('/');

    let filtered_query = query
        .map(|q| {
            let kept: Vec<&str> = q.split('&').filter(|kv| {
                let key = kv.split('=').next().unwrap_or("");
                !is_tracking_param(key)
            }).collect();
            if kept.is_empty() {
                String::new()
            } else {
                format!("?{}", kept.join("&"))
            }
        })
        .unwrap_or_default();

    format!("{}{}", base_trimmed, filtered_query)
}

fn is_tracking_param(key: &str) -> bool {
    key.starts_with("utm_")
        || matches!(
            key,
            "gclid" | "fbclid" | "mc_cid" | "mc_eid" | "ref" | "referrer" | "spm" | "scm" | "igshid"
        )
}

/// Internal duplicate detection logic using provided items.
/// Items inside a group and the group list itself are sorted deterministically
/// so that "keep the first item" means the same thing on every recompute.
pub fn find_duplicates(items: &[TrackedItem]) -> Vec<DuplicateGroup> {
    let mut groups: HashMap<String, Vec<TrackedItem>> = HashMap::new();

    for item in items {
        groups
            .entry(resource_key(item))
            .or_insert_with(Vec::new)
            .push(item.clone());
    }

    let mut groups: Vec<(String, Vec<TrackedItem>)> = groups.into_iter().collect();
    for (_, group_items) in groups.iter_mut() {
        group_items.sort_by(|a, b| {
            b.last_active_at
                .cmp(&a.last_active_at)
                .then_with(|| a.id.cmp(&b.id))
        });
    }
    groups.sort_by(|a, b| a.0.cmp(&b.0));

    groups
        .into_iter()
        .filter(|(_, group_items)| group_items.len() > 1)
        .map(|(key, group_items)| {
            let count = group_items.len();
            let match_type = if key.starts_with("url:") {
                "url_exact"
            } else if key.starts_with("path:") {
                "path_exact"
            } else if key.starts_with("app:") {
                "app_title"
            } else {
                "title_match"
            };

            DuplicateGroup {
                id: Uuid::new_v4().to_string(),
                match_type: match_type.to_string(),
                match_pattern: key,
                items: group_items,
                count,
            }
        })
        .collect()
}

/// Close all items in the matched duplicate groups.
/// Groups are matched by id OR by match_pattern (stable key) so the frontend
/// can reliably reference groups across recomputes.
/// `keep_item_ids` explicitly lists items to keep; when absent, the first
/// item of each (deterministically sorted) group is kept.
/// Returns the count of successfully closed windows.
#[tauri::command]
pub async fn close_duplicates(
    group_ids: Vec<String>,
    keep_item_ids: Option<Vec<String>>,
) -> Result<usize, String> {
    // Recompute groups from live data (keep the raw list: closing an
    // Explorer tab may only close its window when no sibling tabs exist)
    let all_items = db::get_all_tracked_items().await.map_err(|e| e.to_string())?;
    let all_groups = find_duplicates(&all_items);

    let keeps: HashSet<String> = keep_item_ids.unwrap_or_default().into_iter().collect();
    let keep_explicit = !keeps.is_empty();
    let mut closed = 0;

    for group in all_groups {
        if group_ids.contains(&group.id) || group_ids.contains(&group.match_pattern) {
            for (i, item) in group.items.iter().enumerate() {
                let should_keep = if keep_explicit {
                    keeps.contains(&item.id)
                } else {
                    i == 0
                };
                if should_keep {
                    continue;
                }

                let did_close = if item.item_type == ItemType::BrowserTab {
                    // Close browser tab via extension or CDP
                    crate::browser::close_any_tab(&item.id).await
                } else if let Some(hwnd) = item.window_handle {
                    if item.item_type == ItemType::ExplorerWindow
                        && !crate::platform::can_close_explorer_window(hwnd, &all_items)
                    {
                        // Multi-tab window: close just this tab via UIA
                        // (closing the shared HWND would kill sibling tabs)
                        let title = item.title.clone();
                        tokio::task::spawn_blocking(move || {
                            crate::platform::close_explorer_tab(hwnd, &title)
                        })
                        .await
                        .unwrap_or(false)
                    } else {
                        close_single_window(hwnd)
                    }
                } else {
                    false
                };

                // Delete from DB regardless — if window is gone, good;
                // if it's still there, the next scan will pick it up again
                let _ = db::delete_tracked_item(&item.id).await;
                if did_close {
                    closed += 1;
                }
            }

            let _ = db::delete_duplicate_group(&group.id).await;
        }
    }

    Ok(closed)
}

fn close_single_window(hwnd: i64) -> bool {
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
