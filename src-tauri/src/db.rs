use crate::platform::{ItemType, Stats, Task, TrackedItem};
use chrono::Utc;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use sqlx::Row;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use std::time::Duration;
use uuid::Uuid;

static DB_POOL: OnceLock<SqlitePool> = OnceLock::new();

async fn pool() -> &'static SqlitePool {
    DB_POOL.get().expect("Database not initialized")
}

/// Initialize the database and run migrations.
/// `data_dir` should be the OS app-data dir (e.g. %APPDATA%/com.tabflow.app);
/// when unavailable we fall back to the executable directory for dev runs.
pub async fn init_db(data_dir: Option<std::path::PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let db_dir = data_dir.unwrap_or_else(|| {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."))
    });
    std::fs::create_dir_all(&db_dir)?;
    let db_path = db_dir.join("tabflow.db");

    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));

    // Single connection: SQLite allows one writer at a time, and a single
    // connection serializes access without "database is locked" errors.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
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

        -- Assignments reference a stable resource key (normalized URL/path/
        -- process+title), NOT a window instance: window rows come and go with
        -- every scan, while the assignment should survive closing a window and
        -- re-attach when the same resource is opened again.
        CREATE TABLE IF NOT EXISTS item_tasks (
            resource_key TEXT NOT NULL,
            task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            assigned_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (resource_key, task_id)
        );

        CREATE TABLE IF NOT EXISTS duplicate_groups (
            id TEXT PRIMARY KEY,
            match_type TEXT NOT NULL,
            match_pattern TEXT NOT NULL,
            item_count INTEGER NOT NULL DEFAULT 0,
            detected_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        -- Resources the user explicitly removed from tracking. Matched by
        -- the same stable resource key as task assignments, so an ignored
        -- page stays ignored across window instances and app restarts.
        CREATE TABLE IF NOT EXISTS ignored_resources (
            resource_key TEXT PRIMARY KEY,
            title TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        -- User annotations (custom names) per resource key. Shown instead of
        -- the live title; carried over when an instance's key migrates.
        -- EPHEMERAL by design: rows are pruned when the resource is no
        -- longer open (see sync_items), so notes never outlive windows.
        CREATE TABLE IF NOT EXISTS resource_notes (
            resource_key TEXT PRIMARY KEY,
            note TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE INDEX IF NOT EXISTS idx_tracked_items_type ON tracked_items(item_type);
        CREATE INDEX IF NOT EXISTS idx_tracked_items_process ON tracked_items(process_name);
        CREATE INDEX IF NOT EXISTS idx_tracked_items_title ON tracked_items(title);
        CREATE INDEX IF NOT EXISTS idx_item_tasks_task ON item_tasks(task_id);
        "#,
    )
    .execute(&pool)
    .await?;

    // Notes are session-scoped: never resurrect them across app restarts.
    sqlx::query("DELETE FROM resource_notes")
        .execute(&pool)
        .await?;

    DB_POOL
        .set(pool)
        .map_err(|_| "DB already initialized")?;

    Ok(())
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
        icon: None,
        note: None,
        task_ids: Vec::new(),
    }
}

/// Fill `task_ids` from the item_tasks assignments (matched via resource keys).
async fn attach_task_ids(items: &mut Vec<TrackedItem>) {
    let assigns: Vec<(String, String)> =
        match sqlx::query_as("SELECT resource_key, task_id FROM item_tasks")
            .fetch_all(pool().await)
            .await
        {
            Ok(a) => a,
            Err(_) => return,
        };
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for (key, task) in assigns {
        map.entry(key).or_default().push(task);
    }
    for item in items {
        let key = crate::duplicate::resource_key(item);
        if let Some(ids) = map.get(&key) {
            item.task_ids = ids.clone();
        }
    }
}

/// Post-process DB rows into fully-populated items: task assignments, real
/// app icons (icons aren't persisted - they come from the in-memory
/// per-process cache), and the ignore filter - every consumer (list,
/// duplicates, search, stats) reads through this.
async fn hydrate_items(mut items: Vec<TrackedItem>) -> Vec<TrackedItem> {
    attach_task_ids(&mut items).await;
    let notes: HashMap<String, String> =
        match sqlx::query_as("SELECT resource_key, note FROM resource_notes")
            .fetch_all(pool().await)
            .await
        {
            Ok(rows) => rows.into_iter().collect(),
            Err(_) => HashMap::new(),
        };
    let ignored = load_ignored_keys().await;
    if !ignored.is_empty() {
        items.retain(|item| !ignored.contains(&crate::duplicate::resource_key(item)));
    }
    for item in items.iter_mut() {
        let key = crate::duplicate::resource_key(item);
        if let Some(note) = notes.get(&key) {
            item.note = Some(note.clone());
        }
        if item.icon.is_none() {
            item.icon = crate::platform::process_icon(&item.process_name);
        }
    }
    items
}

/// Set (empty string clears) the user note on an item's resource. Notes are
/// ephemeral: sync_items prunes notes whose resource is no longer open, so
/// closing the window (or the app) discards them.
#[tauri::command]
pub async fn set_resource_note(item_id: String, note: String) -> Result<(), String> {
    set_resource_note_inner(&item_id, &note)
        .await
        .map_err(|e| e.to_string())
}

pub async fn set_resource_note_inner(item_id: &str, note: &str) -> Result<(), sqlx::Error> {
    let resource_key = resolve_resource_key(item_id).await?;
    let note = note.trim();
    if note.is_empty() {
        sqlx::query("DELETE FROM resource_notes WHERE resource_key = ?")
            .bind(&resource_key)
            .execute(pool().await)
            .await?;
    } else {
        sqlx::query(
            "INSERT INTO resource_notes (resource_key, note, created_at) \
             VALUES (?, ?, datetime('now')) \
             ON CONFLICT(resource_key) DO UPDATE SET note = excluded.note",
        )
        .bind(&resource_key)
        .bind(note)
        .execute(pool().await)
        .await?;
    }
    Ok(())
}

async fn load_ignored_keys() -> std::collections::HashSet<String> {
    sqlx::query_as::<_, (String,)>("SELECT resource_key FROM ignored_resources")
        .fetch_all(pool().await)
        .await
        .map(|rows| rows.into_iter().map(|(k,)| k).collect())
        .unwrap_or_default()
}

const ITEM_COLUMNS: &str =
    "id, title, url, path, process_name, window_handle, item_type, browser_name, last_active_at";

// ─── Tracked Items ────────────────────────────────────────

/// Order by last_active_at with a deterministic id tiebreaker so that
/// repeated reads of the same data produce the same order (duplicate groups
/// rely on this to keep "keep the first item" stable).
pub async fn get_all_tracked_items() -> Result<Vec<TrackedItem>, sqlx::Error> {
    let rows = sqlx::query(&format!(
        "SELECT {ITEM_COLUMNS} FROM tracked_items ORDER BY last_active_at DESC, id ASC"
    ))
    .fetch_all(pool().await)
    .await?;

    let items: Vec<TrackedItem> = rows.iter().map(|r| row_to_item(r)).collect();
    Ok(hydrate_items(items).await)
}

/// Atomically bring tracked_items in sync with a fresh scan:
/// upsert all live items and delete rows that are no longer open.
/// Runs in a single transaction so concurrent readers never see a
/// half-updated list.
pub async fn sync_items(items: &[TrackedItem]) -> Result<(), sqlx::Error> {
    let mut tx = pool().await.begin().await?;

    // Identity migration: an instance's resource key can change while the
    // instance itself stays open — a tab navigating in place (stable
    // ext_{browser}_{tabId} id) or an app window whose title changed
    // (stable hwnd_ id, e.g. VSCode switching files). Task assignments
    // and the user note are keyed by resource key and would silently
    // detach; carry them over to the new key so tracking follows what
    // the user is doing, not the exact URL/title string. Copy, not move:
    // the old key may still be open in another window/tab.
    let old_rows = sqlx::query(&format!("SELECT {ITEM_COLUMNS} FROM tracked_items"))
        .fetch_all(&mut *tx)
        .await?;
    let old_keys: HashMap<String, String> = old_rows
        .iter()
        .map(|r| {
            let item = row_to_item(r);
            (item.id.clone(), crate::duplicate::resource_key(&item))
        })
        .collect();
    for item in items {
        let new_key = crate::duplicate::resource_key(item);
        let Some(old_key) = old_keys.get(&item.id) else {
            continue;
        };
        if *old_key == new_key {
            continue;
        }
        sqlx::query(
            "INSERT OR IGNORE INTO item_tasks (resource_key, task_id) \
             SELECT ?1, task_id FROM item_tasks WHERE resource_key = ?2",
        )
        .bind(&new_key)
        .bind(old_key)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT OR IGNORE INTO resource_notes (resource_key, note, created_at) \
             SELECT ?1, note, created_at FROM resource_notes WHERE resource_key = ?2",
        )
        .bind(&new_key)
        .bind(old_key)
        .execute(&mut *tx)
        .await?;
    }

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
        .bind(&item.item_type.as_str())
        .bind(&item.browser_name)
        .bind(&item.last_active_at)
        .execute(&mut *tx)
        .await?;
    }

    if items.is_empty() {
        sqlx::query("DELETE FROM tracked_items")
            .execute(&mut *tx)
            .await?;
    } else {
        let placeholders: Vec<&str> = items.iter().map(|_| "?").collect();
        let sql = format!(
            "DELETE FROM tracked_items WHERE id NOT IN ({})",
            placeholders.join(", ")
        );
        let mut query = sqlx::query(&sql);
        for id in items.iter().map(|i| &i.id) {
            query = query.bind(id);
        }
        query.execute(&mut *tx).await?;
    }

    // Notes annotate currently-open resources and are deliberately NOT
    // persistent: prune notes whose resource is no longer live so a note
    // never outlives its window (close it / reopen the page → fresh).
    let live_keys: HashSet<String> = items
        .iter()
        .map(|i| crate::duplicate::resource_key(i))
        .collect();
    let note_keys: Vec<(String,)> =
        sqlx::query_as("SELECT resource_key FROM resource_notes")
            .fetch_all(&mut *tx)
            .await?;
    for (key,) in note_keys {
        if !live_keys.contains(&key) {
            sqlx::query("DELETE FROM resource_notes WHERE resource_key = ?")
                .bind(&key)
                .execute(&mut *tx)
                .await?;
        }
    }

    tx.commit().await
}

pub async fn delete_tracked_item(id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM tracked_items WHERE id = ?")
        .bind(id)
        .execute(pool().await)
        .await?;
    Ok(())
}

pub async fn touch_item(id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE tracked_items SET last_active_at = ? WHERE id = ?")
        .bind(Utc::now().to_rfc3339())
        .bind(id)
        .execute(pool().await)
        .await?;
    Ok(())
}

pub async fn get_tracked_item(id: &str) -> Result<Option<TrackedItem>, sqlx::Error> {
    let row = sqlx::query(&format!(
        "SELECT {ITEM_COLUMNS} FROM tracked_items WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(pool().await)
    .await?;

    Ok(row.as_ref().map(|r| row_to_item(r)))
}

pub async fn search_items(query: &str) -> Result<Vec<TrackedItem>, sqlx::Error> {
    // Escape LIKE wildcards to treat them as literal characters
    let escaped = query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    let pattern = format!("%{}%", escaped);
    let rows = sqlx::query(&format!(
        "SELECT {ITEM_COLUMNS} FROM tracked_items \
         WHERE title LIKE ? ESCAPE '\\' OR url LIKE ? ESCAPE '\\' \
         OR path LIKE ? ESCAPE '\\' OR process_name LIKE ? ESCAPE '\\' \
         ORDER BY last_active_at DESC, id ASC"
    ))
    .bind(&pattern)
    .bind(&pattern)
    .bind(&pattern)
    .bind(&pattern)
    .fetch_all(pool().await)
    .await?;

    let items: Vec<TrackedItem> = rows.iter().map(|r| row_to_item(r)).collect();
    search_note_matches(items, &pattern).await
}

/// Extend text search results with rows whose user note matches the query
/// (notes live on resource keys, which are computed in Rust, so the
/// note-matching half runs outside the LIKE query). Deduped by id.
async fn search_note_matches(
    mut items: Vec<TrackedItem>,
    pattern: &str,
) -> Result<Vec<TrackedItem>, sqlx::Error> {
    let note_keys: HashSet<String> =
        sqlx::query_as::<_, (String,)>("SELECT resource_key FROM resource_notes WHERE note LIKE ? ESCAPE '\\'")
            .bind(pattern)
            .fetch_all(pool().await)
            .await?
            .into_iter()
            .map(|(k,)| k)
            .collect();
    if !note_keys.is_empty() {
        let present: HashSet<String> = items.iter().map(|i| i.id.clone()).collect();
        let all = sqlx::query(&format!("SELECT {ITEM_COLUMNS} FROM tracked_items"))
            .fetch_all(pool().await)
            .await?;
        for row in &all {
            let item = row_to_item(row);
            if !present.contains(&item.id)
                && note_keys.contains(&crate::duplicate::resource_key(&item))
            {
                items.push(item);
            }
        }
    }
    items.sort_by(|a, b| b.last_active_at.cmp(&a.last_active_at).then(a.id.cmp(&b.id)));
    Ok(hydrate_items(items).await)
}

// ─── Duplicate Groups ─────────────────────────────────────

/// duplicate_groups is derived data: mirror the current detection result
/// (clear + reinsert in one transaction) instead of appending, so the table
/// never accumulates stale rows and get_stats stays correct.
pub async fn detect_and_store_duplicates() -> Result<(), sqlx::Error> {
    use crate::duplicate::find_duplicates;

    let groups = find_duplicates(&get_all_tracked_items().await?);

    let mut tx = pool().await.begin().await?;
    sqlx::query("DELETE FROM duplicate_groups")
        .execute(&mut *tx)
        .await?;

    for group in &groups {
        sqlx::query(
            "INSERT INTO duplicate_groups (id, match_type, match_pattern, item_count, detected_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&group.id)
        .bind(&group.match_type)
        .bind(&group.match_pattern)
        .bind(group.count as i32)
        .bind(Utc::now().to_rfc3339())
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await
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
    let rows = sqlx::query("SELECT id, name, color, sort_order FROM tasks ORDER BY sort_order ASC")
        .fetch_all(pool().await)
        .await?;

    let assigns: Vec<(String, String)> =
        sqlx::query_as("SELECT task_id, resource_key FROM item_tasks")
            .fetch_all(pool().await)
            .await?;

    // Count live items matching each task's resource keys
    let mut live_counts: HashMap<String, i32> = HashMap::new();
    for item in get_all_tracked_items().await? {
        *live_counts
            .entry(crate::duplicate::resource_key(&item))
            .or_insert(0) += 1;
    }

    Ok(rows
        .iter()
        .map(|r| {
            let id: String = r.get("id");
            let count = assigns
                .iter()
                .filter(|(tid, key)| tid == &id && live_counts.contains_key(key))
                .count() as i32;
            Task {
                id,
                name: r.get("name"),
                color: r.get("color"),
                sort_order: r.get("sort_order"),
                item_count: Some(count),
            }
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

    let row = sqlx::query_as::<_, (String, String, String, i32)>(
        "SELECT id, name, color, sort_order FROM tasks WHERE id = ?",
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

/// Resolve an item to its stable resource key. Fails when the window is
/// already gone (row deleted by the scan).
async fn resolve_resource_key(item_id: &str) -> Result<String, sqlx::Error> {
    match get_tracked_item(item_id).await? {
        Some(item) => Ok(crate::duplicate::resource_key(&item)),
        None => Err(sqlx::Error::RowNotFound),
    }
}

pub async fn assign_item_to_task(item_id: &str, task_id: &str) -> Result<(), sqlx::Error> {
    let resource_key = resolve_resource_key(item_id).await?;
    sqlx::query("INSERT OR IGNORE INTO item_tasks (resource_key, task_id) VALUES (?, ?)")
        .bind(&resource_key)
        .bind(task_id)
        .execute(pool().await)
        .await?;
    Ok(())
}

pub async fn unassign_item_from_task(item_id: &str, task_id: &str) -> Result<(), sqlx::Error> {
    let resource_key = resolve_resource_key(item_id).await?;
    sqlx::query("DELETE FROM item_tasks WHERE resource_key = ? AND task_id = ?")
        .bind(&resource_key)
        .bind(task_id)
        .execute(pool().await)
        .await?;
    Ok(())
}

/// Live tracked items currently matching a task's assigned resource keys.
pub async fn get_task_items(task_id: &str) -> Result<Vec<TrackedItem>, sqlx::Error> {
    let keys: Vec<(String,)> =
        sqlx::query_as("SELECT resource_key FROM item_tasks WHERE task_id = ?")
            .bind(task_id)
            .fetch_all(pool().await)
            .await?;
    let key_set: HashSet<String> = keys.into_iter().map(|(k,)| k).collect();

    // get_all_tracked_items already attached task_ids. Return ONE row per
    // tracked resource: duplicate instances of the same page share the
    // resource key and would all flood the task view otherwise. Items come
    // back ordered by last_active_at DESC, so the first is the most recent
    // live instance of the resource.
    let items = get_all_tracked_items().await?;
    let mut seen: HashSet<String> = HashSet::new();
    Ok(items
        .into_iter()
        .filter(|i| {
            let key = crate::duplicate::resource_key(i);
            key_set.contains(&key) && seen.insert(key)
        })
        .collect())
}

// ─── Ignored Resources ────────────────────────────────────

/// Stop tracking an item: its resource key goes onto the ignore list, so
/// the page disappears from the list/duplicates/stats until unignored
/// (even after the window is closed and reopened).
#[tauri::command]
pub async fn ignore_item(item_id: String) -> Result<(), String> {
    let item = get_tracked_item(&item_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("item not found: {}", item_id))?;
    let key = crate::duplicate::resource_key(&item);

    sqlx::query("INSERT OR IGNORE INTO ignored_resources (resource_key, title) VALUES (?, ?)")
        .bind(&key)
        .bind(&item.title)
        .execute(pool().await)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Resume tracking a previously ignored resource.
#[tauri::command]
pub async fn unignore_resource(resource_key: String) -> Result<(), String> {
    sqlx::query("DELETE FROM ignored_resources WHERE resource_key = ?")
        .bind(&resource_key)
        .execute(pool().await)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// All ignored resources (for the management view), newest first.
#[tauri::command]
pub async fn get_ignored_resources() -> Result<Vec<crate::platform::IgnoredResource>, String> {
    sqlx::query_as::<_, (String, String, String)>(
        "SELECT resource_key, title, created_at FROM ignored_resources ORDER BY created_at DESC, resource_key ASC",
    )
    .fetch_all(pool().await)
    .await
    .map_err(|e| e.to_string())
    .map(|rows| {
        rows.into_iter()
            .map(|(resource_key, title, created_at)| crate::platform::IgnoredResource {
                resource_key,
                title,
                created_at,
            })
            .collect()
    })
}

// ─── Stats ────────────────────────────────────────────────

#[tauri::command]
pub async fn get_stats() -> Result<Stats, String> {
    // Count from the hydrated (ignore-filtered) item list so ignored
    // resources stay out of the numbers, exactly like out of the list.
    let items = get_all_tracked_items().await.map_err(|e| e.to_string())?;
    let mut browser = 0i64;
    let mut explorer = 0i64;
    let mut apps = 0i64;
    for item in &items {
        match item.item_type {
            crate::platform::ItemType::BrowserTab => browser += 1,
            crate::platform::ItemType::ExplorerWindow => explorer += 1,
            crate::platform::ItemType::AppWindow => apps += 1,
        }
    }

    let tasks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks")
        .fetch_one(pool().await)
        .await
        .map_err(|e| e.to_string())?;

    let dupes: i64 =
        sqlx::query_scalar("SELECT COALESCE(SUM(item_count - 1), 0) FROM duplicate_groups")
            .fetch_one(pool().await)
            .await
            .map_err(|e| e.to_string())?;

    Ok(Stats {
        total_items: items.len() as i64,
        duplicate_count: dupes,
        browser_tabs: browser,
        explorer_windows: explorer,
        app_windows: apps,
        active_tasks: tasks,
    })
}
