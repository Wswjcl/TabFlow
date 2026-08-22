use crate::platform::{DuplicateGroup, ItemType, Stats, Task, TrackedItem};
use chrono::Utc;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use sqlx::Row;
use std::sync::OnceLock;
use uuid::Uuid;

static DB_POOL: OnceLock<SqlitePool> = OnceLock::new();

async fn pool() -> &'static SqlitePool {
    DB_POOL.get().expect("Database not initialized")
}

/// Initialize the database and run migrations
pub async fn init_db() -> Result<(), Box<dyn std::error::Error>> {
    let db_dir = get_data_dir();
    std::fs::create_dir_all(&db_dir).ok();
    let db_path = db_dir.join("tabflow.db");

    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS tracked_items (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            url TEXT,
            path TEXT,
            process_name TEXT NOT NULL,
            window_handle INTEGER,
            item_type TEXT NOT NULL DEFAULT 'app_window',
            browser_name TEXT,
            last_active_at TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS tasks (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            color TEXT NOT NULL DEFAULT '#6366f1',
            sort_order INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS item_tasks (
            item_id TEXT NOT NULL REFERENCES tracked_items(id) ON DELETE CASCADE,
            task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            assigned_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (item_id, task_id)
        );

        CREATE TABLE IF NOT EXISTS duplicate_groups (
            id TEXT PRIMARY KEY,
            match_type TEXT NOT NULL,
            match_pattern TEXT NOT NULL,
            item_count INTEGER NOT NULL DEFAULT 0,
            detected_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE INDEX IF NOT EXISTS idx_tracked_items_type ON tracked_items(item_type);
        CREATE INDEX IF NOT EXISTS idx_tracked_items_process ON tracked_items(process_name);
        CREATE INDEX IF NOT EXISTS idx_tracked_items_title ON tracked_items(title);
        CREATE INDEX IF NOT EXISTS idx_item_tasks_task ON item_tasks(task_id);
        "#,
    )
    .execute(&pool)
    .await?;

    DB_POOL
        .set(pool)
        .map_err(|_| "DB already initialized")?;

    Ok(())
}

fn get_data_dir() -> std::path::PathBuf {
    // Use the executable's directory for the database
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            return exe_dir.to_path_buf();
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let dir = std::path::PathBuf::from(appdata).join("TabFlow");
            std::fs::create_dir_all(&dir).ok();
            return dir;
        }
    }

    std::path::PathBuf::from(".")
}

// ─── Helper: row → TrackedItem ───────────────────────────

fn row_to_item(row: &sqlx::sqlite::SqliteRow) -> TrackedItem {
    TrackedItem {
        id: row.get("id"),
        title: row.get("title"),
        url: row.get("url"),
        path: row.get("path"),
        process_name: row.get("process_name"),
        window_handle: row.get("window_handle"),
        item_type: ItemType::from_str(row.get::<String, _>("item_type").as_str()),
        browser_name: row.get("browser_name"),
        last_active_at: row.get("last_active_at"),
    }
}

// ─── Tracked Items ────────────────────────────────────────

pub async fn get_all_tracked_items() -> Result<Vec<TrackedItem>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, title, url, path, process_name, window_handle, item_type, browser_name, last_active_at FROM tracked_items ORDER BY last_active_at DESC"
    )
    .fetch_all(pool().await)
    .await?;

    Ok(rows.iter().map(|r| row_to_item(r)).collect())
}

pub async fn upsert_windows(items: &[TrackedItem]) -> Result<(), sqlx::Error> {
    for item in items {
        sqlx::query(
            r#"
            INSERT INTO tracked_items (id, title, url, path, process_name, window_handle, item_type, browser_name, last_active_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                url = excluded.url,
                path = excluded.path,
                last_active_at = excluded.last_active_at
            "#,
        )
        .bind(&item.id)
        .bind(&item.title)
        .bind(&item.url)
        .bind(&item.path)
        .bind(&item.process_name)
        .bind(item.window_handle)
        .bind(item.item_type.as_str())
        .bind(&item.browser_name)
        .bind(&item.last_active_at)
        .execute(pool().await)
        .await?;
    }
    Ok(())
}

pub async fn delete_tracked_item(id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM tracked_items WHERE id = ?")
        .bind(id)
        .execute(pool().await)
        .await?;
    Ok(())
}

/// Remove items that are no longer open (stale)
pub async fn cleanup_stale_items(current_ids: &[String]) -> Result<usize, sqlx::Error> {
    if current_ids.is_empty() {
        // No windows open → delete everything
        let result = sqlx::query("DELETE FROM tracked_items")
            .execute(pool().await)
            .await?;
        return Ok(result.rows_affected() as usize);
    }

    // Delete items whose IDs are NOT in the current live list
    // Build dynamic query: DELETE FROM tracked_items WHERE id NOT IN (?, ?, ...)
    let placeholders: Vec<String> = current_ids.iter().map(|_| "?".to_string()).collect();
    let sql = format!(
        "DELETE FROM tracked_items WHERE id NOT IN ({})",
        placeholders.join(", ")
    );

    let mut query = sqlx::query(&sql);
    for id in current_ids {
        query = query.bind(id);
    }

    let result = query.execute(pool().await).await?;
    Ok(result.rows_affected() as usize)
}

pub async fn touch_item(id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE tracked_items SET last_active_at = ? WHERE id = ?")
        .bind(Utc::now().to_rfc3339())
        .bind(id)
        .execute(pool().await)
        .await?;
    Ok(())
}

pub async fn search_items(query: &str) -> Result<Vec<TrackedItem>, sqlx::Error> {
    // Escape LIKE wildcards to treat them as literal characters
    let escaped = query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    let pattern = format!("%{}%", escaped);
    let rows = sqlx::query(
        "SELECT id, title, url, path, process_name, window_handle, item_type, browser_name, last_active_at FROM tracked_items WHERE title LIKE ? ESCAPE '\\' OR url LIKE ? ESCAPE '\\' OR path LIKE ? ESCAPE '\\' OR process_name LIKE ? ESCAPE '\\' ORDER BY last_active_at DESC"
    )
    .bind(&pattern)
    .bind(&pattern)
    .bind(&pattern)
    .bind(&pattern)
    .fetch_all(pool().await)
    .await?;

    Ok(rows.iter().map(|r| row_to_item(r)).collect())
}

// ─── Duplicate Groups ─────────────────────────────────────

pub async fn detect_and_store_duplicates() -> Result<Vec<DuplicateGroup>, sqlx::Error> {
    use crate::duplicate::find_duplicates;

    let items = get_all_tracked_items().await?;
    let groups = find_duplicates(&items);

    for group in &groups {
        sqlx::query(
            "INSERT OR REPLACE INTO duplicate_groups (id, match_type, match_pattern, item_count, detected_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&group.id)
        .bind(&group.match_type)
        .bind(&group.match_pattern)
        .bind(group.count as i32)
        .bind(Utc::now().to_rfc3339())
        .execute(pool().await)
        .await?;
    }

    Ok(groups)
}

pub async fn get_duplicate_groups() -> Result<Vec<DuplicateGroup>, sqlx::Error> {
    let items = get_all_tracked_items().await?;
    Ok(crate::duplicate::find_duplicates(&items))
}

pub async fn delete_duplicate_group(id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM duplicate_groups WHERE id = ?")
        .bind(id)
        .execute(pool().await)
        .await?;
    Ok(())
}

// ─── Tasks ────────────────────────────────────────────────

pub async fn get_all_tasks() -> Result<Vec<Task>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT t.id, t.name, t.color, t.sort_order,
               COUNT(it.item_id) as item_count
        FROM tasks t
        LEFT JOIN item_tasks it ON t.id = it.task_id
        GROUP BY t.id
        ORDER BY t.sort_order ASC
        "#,
    )
    .fetch_all(pool().await)
    .await?;

    Ok(rows
        .iter()
        .map(|r| Task {
            id: r.get("id"),
            name: r.get("name"),
            color: r.get("color"),
            sort_order: r.get("sort_order"),
            item_count: Some(r.get::<i64, _>("item_count") as i32),
        })
        .collect())
}

pub async fn create_task(name: &str, color: &str) -> Result<Task, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO tasks (id, name, color, sort_order) VALUES (?, ?, ?, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM tasks))",
    )
    .bind(&id)
    .bind(name)
    .bind(color)
    .execute(pool().await)
    .await?;

    // Read back the actual sort_order from DB instead of hardcoding 0
    let row = sqlx::query_as::<_, (String, String, String, i32, i64)>(
        "SELECT id, name, color, sort_order, 0 FROM tasks WHERE id = ?",
    )
    .bind(&id)
    .fetch_one(pool().await)
    .await?;

    Ok(Task {
        id: row.0,
        name: row.1,
        color: row.2,
        sort_order: row.3,
        item_count: Some(0),
    })
}

pub async fn update_task(id: &str, name: &str, color: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE tasks SET name = ?, color = ? WHERE id = ?")
        .bind(name)
        .bind(color)
        .bind(id)
        .execute(pool().await)
        .await?;
    Ok(())
}

pub async fn delete_task(id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM tasks WHERE id = ?")
        .bind(id)
        .execute(pool().await)
        .await?;
    Ok(())
}

pub async fn assign_item_to_task(item_id: &str, task_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT OR IGNORE INTO item_tasks (item_id, task_id) VALUES (?, ?)")
        .bind(item_id)
        .bind(task_id)
        .execute(pool().await)
        .await?;
    Ok(())
}

pub async fn unassign_item_from_task(item_id: &str, task_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM item_tasks WHERE item_id = ? AND task_id = ?")
        .bind(item_id)
        .bind(task_id)
        .execute(pool().await)
        .await?;
    Ok(())
}

pub async fn get_task_items(task_id: &str) -> Result<Vec<TrackedItem>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT ti.id, ti.title, ti.url, ti.path, ti.process_name,
               ti.window_handle, ti.item_type, ti.browser_name, ti.last_active_at
        FROM tracked_items ti
        JOIN item_tasks it ON ti.id = it.item_id
        WHERE it.task_id = ?
        ORDER BY ti.last_active_at DESC
        "#,
    )
    .bind(task_id)
    .fetch_all(pool().await)
    .await?;

    Ok(rows.iter().map(|r| row_to_item(r)).collect())
}

// ─── Stats ────────────────────────────────────────────────

#[tauri::command]
pub async fn get_stats() -> Result<Stats, String> {
    let pool = pool().await;

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracked_items")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;

    let browser: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tracked_items WHERE item_type = 'browser_tab'")
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;

    let explorer: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tracked_items WHERE item_type = 'explorer_window'")
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;

    let apps: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tracked_items WHERE item_type = 'app_window'")
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;

    let tasks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;

    let dupes: i64 =
        sqlx::query_scalar("SELECT COALESCE(SUM(item_count - 1), 0) FROM duplicate_groups")
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;

    Ok(Stats {
        total_items: total,
        duplicate_count: dupes,
        browser_tabs: browser,
        explorer_windows: explorer,
        app_windows: apps,
        active_tasks: tasks,
    })
}