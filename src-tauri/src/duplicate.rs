use crate::cdp;
use crate::platform::{DuplicateGroup, TrackedItem, ItemType};
use crate::db;
use uuid::Uuid;
use std::collections::HashMap;

/// Detect duplicate windows/tabs from DB
#[tauri::command]
pub async fn detect_duplicates() -> Result<Vec<DuplicateGroup>, String> {
    db::detect_and_store_duplicates()
        .await
        .map_err(|e| e.to_string())
}

/// Internal duplicate detection logic using provided items
pub fn find_duplicates(items: &[TrackedItem]) -> Vec<DuplicateGroup> {
    let mut groups: HashMap<String, Vec<TrackedItem>> = HashMap::new();

    for item in items {
        let key = match &item.item_type {
            ItemType::BrowserTab => {
                if let Some(ref url) = item.url {
                    let normalized = url.trim_end_matches('/').to_lowercase();
                    format!("url:{}", normalized)
                } else {
                    // No URL extracted → don't group with anything
                    // Each untagged browser window is unique
                    format!("browser_standalone:{}", item.id)
                }
            }
            ItemType::ExplorerWindow => {
                if let Some(ref path) = item.path {
                    format!("path:{}", path.to_lowercase())
                } else {
                    format!("explorer_standalone:{}", item.id)
                }
            }
            ItemType::AppWindow => {
                // Only group app windows if title + process both match
                format!("app:{}:{}", item.process_name, item.title.to_lowercase())
            }
        };

        groups.entry(key).or_insert_with(Vec::new).push(item.clone());
    }

    groups
        .into_iter()
        .filter(|(_, items)| items.len() > 1)
        .map(|(key, items)| {
            let count = items.len();
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
                items,
                count,
            }
        })
        .collect()
}

/// Close all items in a duplicate group except the first one.
/// Uses match_pattern (stable key) instead of group id (which is a random UUID
/// regenerated on every call to find_duplicates) so that the frontend can
/// reliably reference groups across calls.
/// Returns the count of successfully closed windows.
#[tauri::command]
pub async fn close_duplicates(
    group_ids: Vec<String>,
    keep_indices: Option<Vec<usize>>,
) -> Result<usize, String> {
    // Recompute groups from live data
    let all_groups = crate::duplicate::find_duplicates(
        &db::get_all_tracked_items().await.map_err(|e| e.to_string())?,
    );

    let mut closed = 0;

    for group in all_groups {
        // Match by id OR by match_pattern — the id is a random UUID that
        // changes on every find_duplicates() call, so we also accept
        // match_pattern which is a stable key derived from URL/path/title.
        if group_ids.contains(&group.id) || group_ids.contains(&group.match_pattern) {
            let keep_idx = keep_indices
                .as_ref()
                .and_then(|indices| indices.first().copied())
                .unwrap_or(0);

            for (i, item) in group.items.iter().enumerate() {
                if i != keep_idx {
                    let did_close = if item.item_type == ItemType::BrowserTab {
                        // Close browser tab via CDP
                        cdp::close_cdp_tab(&item.id).await
                    } else if let Some(hwnd) = item.window_handle {
                        close_single_window(hwnd)
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

            // Simulate clicking the X button (most reliable)
            let _ = SendMessageW(h, WM_SYSCOMMAND, WPARAM(0xF060), LPARAM(0)); // SC_CLOSE = 0xF060

            true
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = hwnd;
        false
    }
}