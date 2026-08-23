// ─── Shared types (mirrors Rust platform::mod) ─────

export interface TrackedItem {
  id: string;
  title: string;
  url: string | null;
  path: string | null;
  process_name: string;
  window_handle: number | null;
  item_type: "browser_tab" | "explorer_window" | "app_window";
  browser_name: string | null;
  last_active_at: string;
  /** Real app icon as a PNG data URL (null → emoji fallback) */
  icon: string | null;
  /** Task ids assigned to this item's resource (backend-attached) */
  task_ids: string[];
}

export interface DuplicateGroup {
  id: string;
  match_type: string;
  match_pattern: string;
  items: TrackedItem[];
  count: number;
}

export interface Task {
  id: string;
  name: string;
  color: string;
  sort_order: number;
  item_count: number | null;
}

export interface Stats {
  total_items: number;
  duplicate_count: number;
  browser_tabs: number;
  explorer_windows: number;
  app_windows: number;
  active_tasks: number;
}

// ─── Extension channel ─────────────────────────────

export interface ConnectedBrowser {
  browser: string;
  tabCount: number;
}

export interface ExtensionStatus {
  port: number;
  token: string;
  connected: ConnectedBrowser[];
}

// ─── Item type helpers ─────────────────────────────

// Sidebar type-filter pseudo task ids (kept distinct from real task UUIDs)
export const FILTER_BROWSER = "__filter_browser";
export const FILTER_EXPLORER = "__filter_explorer";
export const FILTER_APP = "__filter_app";

export function isFilterId(id: string | null): id is string {
  return id != null && id.startsWith("__filter_");
}

export function itemTypeLabel(type: string): string {
  switch (type) {
    case "browser_tab":
      return "浏览器";
    case "explorer_window":
      return "文件夹";
    case "app_window":
      return "应用";
    default:
      return type;
  }
}

export function itemTypeIcon(type: string): string {
  switch (type) {
    case "browser_tab":
      return "🌐";
    case "explorer_window":
      return "📂";
    case "app_window":
      return "🖥️";
    default:
      return "📄";
  }
}

export function browserIcon(name: string | null): string {
  switch (name) {
    case "chrome":
      return "🔵";
    case "edge":
      return "🟢";
    case "firefox":
      return "🟠";
    default:
      return "🌐";
  }
}

// Task color palette
export const TASK_COLORS = [
  "#6366f1", // indigo
  "#8b5cf6", // violet
  "#ec4899", // pink
  "#f43f5e", // rose
  "#f97316", // orange
  "#eab308", // yellow
  "#22c55e", // green
  "#14b8a6", // teal
  "#06b6d4", // cyan
  "#3b82f6", // blue
];