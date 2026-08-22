use crate::platform::TrackedItem;
use crate::db;

/// Search tracked items by query string.
/// Matches against title, URL, path, process name, and task name.
#[tauri::command]
pub async fn search_items(query: String) -> Result<Vec<TrackedItem>, String> {
    if query.trim().is_empty() {
        return db::get_all_tracked_items().await.map_err(|e| e.to_string());
    }

    db::search_items(&query)
        .await
        .map_err(|e| e.to_string())
}

/// Score how well an item matches a query (for sorting results)
pub fn match_score(item: &TrackedItem, query: &str) -> i32 {
    let query_lower = query.to_lowercase();
    let mut score = 0;

    // Exact title match
    if item.title.to_lowercase() == query_lower {
        score += 100;
    }
    // Title contains query
    if item.title.to_lowercase().contains(&query_lower) {
        score += 50;
    }
    // Title starts with query
    if item.title.to_lowercase().starts_with(&query_lower) {
        score += 30;
    }

    // URL matches
    if let Some(ref url) = item.url {
        if url.to_lowercase().contains(&query_lower) {
            score += 40;
        }
    }

    // Path matches
    if let Some(ref path) = item.path {
        if path.to_lowercase().contains(&query_lower) {
            score += 40;
        }
    }

    // Process name matches
    if item.process_name.to_lowercase().contains(&query_lower) {
        score += 20;
    }

    score
}