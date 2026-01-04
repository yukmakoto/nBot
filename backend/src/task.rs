use crate::models::{BackgroundTask, SharedState};
use axum::extract::{Path, State};
use axum::Json;
use serde_json::json;

pub async fn list_tasks_handler(State(state): State<SharedState>) -> Json<Vec<BackgroundTask>> {
    let mut tasks: Vec<BackgroundTask> = state.tasks.iter().map(|t| t.value().clone()).collect();
    tasks.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Json(tasks)
}

pub async fn delete_task_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    state.tasks.remove(&id);
    Json(json!({ "status": "success" }))
}
