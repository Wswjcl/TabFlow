use serde::{Deserialize, Serialize};

/// Represents a tracked window or tab
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedItem {
    pub id: String,
    pub title: String,
    pub url: Option<String>,
    pub path: Option<String>,
    pub process_name: String,
    pub window_handle: Option<i64>,
    pub item_type: ItemType,
    pub browser_name: Option<String>,
    pub last_active_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ItemType {
    BrowserTab,
    ExplorerWindow,
    AppWindow,
}

impl ItemType {
    pub fn as_str(&self) -> &str {
        match self {
            ItemType::BrowserTab => "browser_tab",
            ItemType::ExplorerWindow => "explorer_window",
            ItemType::AppWindow => "app_window",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "browser_tab" => ItemType::BrowserTab,
            "explorer_window" => ItemType::ExplorerWindow,
            _ => ItemType::AppWindow,
        }
    }
}

/// Duplicate group info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateGroup {
    pub id: String,
    pub match_type: String,
    pub match_pattern: String,
    pub items: Vec<TrackedItem>,
    pub count: usize,
}

/// Task / category
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub color: String,
    pub sort_order: i32,
    pub item_count: Option<i32>,
}

/// Stats overview
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stats {
    pub total_items: i64,
    pub duplicate_count: i64,
    pub browser_tabs: i64,
    pub explorer_windows: i64,
    pub app_windows: i64,
    pub active_tasks: i64,
}

// ─── Platform re-exports ─────────────────────────────────

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{enumerate_windows, find_window_handle_by_process};

#[cfg(target_os = "windows")]
mod explorer;
#[cfg(target_os = "windows")]
pub use explorer::{
    can_close_explorer_window, close_explorer_tab, enumerate_explorer_items,
};

#[cfg(not(target_os = "windows"))]
/// COM enumeration unavailable outside Windows → Err triggers the
/// EnumWindows fallback in the caller.
pub fn enumerate_explorer_items() -> Result<Vec<TrackedItem>, ()> {
    Err(())
}

#[cfg(not(target_os = "windows"))]
pub fn close_explorer_tab(_hwnd: i64, _tab_title: &str) -> bool {
    false
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::enumerate_windows;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn enumerate_windows() -> Vec<TrackedItem> {
    vec![]
}

#[cfg(not(target_os = "windows"))]
pub fn find_window_handle_by_process(_image_name: &str) -> Option<i64> {
    None
}