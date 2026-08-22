use crate::platform::{Task, TrackedItem};
use crate::db;

/// Get all tasks
#[tauri::command]
pub async fn get_all_tasks() -> Result<Vec<Task>, String> {
    db::get_all_tasks().await.map_err(|e| e.to_string())
}

/// Create a new task
#[tauri::command]
pub async fn create_task(name: String, color: String) -> Result<Task, String> {
    db::create_task(&name, &color)
        .await
        .map_err(|e| e.to_string())
}

/// Update a task
#[tauri::command]
pub async fn update_task(id: String, name: String, color: String) -> Result<(), String> {
    db::update_task(&id, &name, &color)
        .await
        .map_err(|e| e.to_string())
}

/// Delete a task
#[tauri::command]
pub async fn delete_task(id: String) -> Result<(), String> {
    db::delete_task(&id).await.map_err(|e| e.to_string())
}

/// Assign a tracked item to a task
#[tauri::command]
pub async fn assign_item_to_task(item_id: String, task_id: String) -> Result<(), String> {
    db::assign_item_to_task(&item_id, &task_id)
        .await
        .map_err(|e| e.to_string())
}

/// Unassign a tracked item from a task
#[tauri::command]
pub async fn unassign_item_from_task(item_id: String, task_id: String) -> Result<(), String> {
    db::unassign_item_from_task(&item_id, &task_id)
        .await
        .map_err(|e| e.to_string())
}

/// Get items assigned to a specific task
#[tauri::command]
pub async fn get_task_items(task_id: String) -> Result<Vec<TrackedItem>, String> {
    db::get_task_items(&task_id)
        .await
        .map_err(|e| e.to_string())
}