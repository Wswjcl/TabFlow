use crate::platform::TrackedItem;
use crate::db;

/// Search tracked items by query string.
/// Matches against title, URL, path, and process name.
#[tauri::command]
pub async fn search_items(query: String) -> Result<Vec<TrackedItem>, String> {
    if query.trim().is_empty() {
        return db::get_all_tracked_items().await.map_err(|e| e.to_string());
    }

    db::search_items(&query)
        .await
        .map_err(|e| e.to_string())
}
