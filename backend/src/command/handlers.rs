use super::{Command, CommandAction, CommandParam};
use crate::models::AppState;
use axum::extract::{Json, Path, State};
use std::sync::Arc;

type SharedState = Arc<AppState>;

pub async fn list_commands_handler(State(state): State<SharedState>) -> Json<Vec<Command>> {
    Json(state.commands.list())
}

pub async fn get_command_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    match state.commands.get(&id) {
        Some(cmd) => Json(serde_json::json!({ "status": "success", "command": cmd })),
        None => Json(serde_json::json!({ "status": "error", "message": "指令不存在" })),
    }
}

#[derive(serde::Deserialize)]
pub struct CreateCommandPayload {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub pattern: Option<String>,
    pub description: String,
    #[serde(default)]
    pub action_value: String,
    #[serde(default)]
    pub params: Vec<CommandParam>,
}

pub async fn create_command_handler(
    State(state): State<SharedState>,
    Json(payload): Json<CreateCommandPayload>,
) -> Json<serde_json::Value> {
    let id = payload.name.to_lowercase().replace(" ", "_");
    let cmd = Command {
        id: id.clone(),
        name: payload.name,
        aliases: payload.aliases,
        pattern: payload.pattern,
        description: payload.description,
        is_builtin: false,
        action: CommandAction::Custom(payload.action_value),
        subcommands: vec![],
        params: payload.params,
        category: "其他".to_string(),
        config: serde_json::json!({}),
    };

    match state.commands.create(cmd) {
        Ok(_) => Json(serde_json::json!({ "status": "success", "id": id })),
        Err(e) => Json(serde_json::json!({ "status": "error", "message": e })),
    }
}

pub async fn update_command_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(updates): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    match state.commands.update(&id, updates) {
        Ok(_) => Json(serde_json::json!({ "status": "success" })),
        Err(e) => Json(serde_json::json!({ "status": "error", "message": e })),
    }
}

pub async fn delete_command_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    match state.commands.delete(&id) {
        Ok(_) => Json(serde_json::json!({ "status": "success" })),
        Err(e) => Json(serde_json::json!({ "status": "error", "message": e })),
    }
}
