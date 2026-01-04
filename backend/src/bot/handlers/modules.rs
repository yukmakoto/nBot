use crate::models::{BotModuleConfig, SharedState};
use crate::module::get_effective_module;
use crate::persistence::save_bots;
use axum::extract::{Json, Path, State};
use serde_json::json;

#[derive(serde::Deserialize)]
pub struct UpdateBotModulePayload {
    pub module_id: String,
    pub enabled: Option<bool>,
    pub config: Option<serde_json::Value>,
}

pub async fn update_bot_module_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateBotModulePayload>,
) -> Json<serde_json::Value> {
    if let Some(mut bot) = state.bots.get_mut(&id) {
        if payload.enabled.is_none() && payload.config.is_none() {
            return Json(json!({
                "status": "error",
                "message": "No changes provided"
            }));
        }

        let module_config = bot
            .modules_config
            .entry(payload.module_id.clone())
            .or_insert_with(BotModuleConfig::default);
        if let Some(enabled) = payload.enabled {
            module_config.enabled = Some(enabled);
        }
        if let Some(config) = payload.config {
            module_config.config = config;
        }
        drop(bot);
        save_bots(&state.bots);
        Json(serde_json::json!({ "status": "success" }))
    } else {
        Json(serde_json::json!({ "status": "error", "message": "Bot not found" }))
    }
}

pub async fn list_bot_effective_modules_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    if state.bots.get(&id).is_none() {
        return Json(json!({ "status": "error", "message": "Bot not found" }));
    }

    let modules = state
        .modules
        .list()
        .into_iter()
        .filter_map(|m| get_effective_module(&state, &id, &m.id))
        .collect::<Vec<_>>();

    Json(json!({ "status": "success", "modules": modules }))
}

pub async fn get_bot_effective_module_handler(
    State(state): State<SharedState>,
    Path((id, module_id)): Path<(String, String)>,
) -> Json<serde_json::Value> {
    if state.bots.get(&id).is_none() {
        return Json(json!({ "status": "error", "message": "Bot not found" }));
    }

    match get_effective_module(&state, &id, &module_id) {
        Some(module) => Json(json!({ "status": "success", "module": module })),
        None => Json(json!({ "status": "error", "message": "Module not found" })),
    }
}

pub async fn delete_bot_module_override_handler(
    State(state): State<SharedState>,
    Path((id, module_id)): Path<(String, String)>,
) -> Json<serde_json::Value> {
    if let Some(mut bot) = state.bots.get_mut(&id) {
        bot.modules_config.remove(&module_id);
        drop(bot);
        save_bots(&state.bots);
        Json(json!({ "status": "success" }))
    } else {
        Json(json!({ "status": "error", "message": "Bot not found" }))
    }
}
