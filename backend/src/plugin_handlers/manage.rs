use crate::models::SharedState;
use crate::plugin::InstalledPlugin;
use axum::extract::{Json, Path, State};
use serde_json::json;
use tracing::warn;

use super::commands::register_plugin_commands;

pub async fn list_installed_handler(
    State(state): State<SharedState>,
) -> Json<Vec<InstalledPlugin>> {
    let mut plugins = state.plugins.list();
    plugins.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
    Json(plugins)
}

pub async fn enable_plugin_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    // Load first, then persist enabled=true to keep state consistent on failures.
    let plugin = match state.plugins.get(&id) {
        Some(p) => p,
        None => {
            return Json(json!({
                "status": "error",
                "message": format!("插件 {} 未找到", id)
            }))
        }
    };

    if !state.plugin_manager.is_loaded(&id) {
        if let Err(e) = state.plugin_manager.load(&plugin).await {
            return Json(json!({
                "status": "error",
                "message": format!("加载插件失败: {}", e)
            }));
        }
    }

    if let Err(e) = state.plugins.enable(&id) {
        // Best-effort rollback to avoid "enabled but loaded" mismatch.
        if state.plugin_manager.is_loaded(&id) {
            let _ = state.plugin_manager.unload(&id).await;
        }
        state.commands.unregister_plugin_commands(&id);
        return Json(json!({ "status": "error", "message": e }));
    }

    // Ensure commands are in sync (idempotent).
    if let Some(plugin) = state.plugins.get(&id) {
        if plugin.enabled {
            register_plugin_commands(&state.commands, &plugin);
        } else {
            state.commands.unregister_plugin_commands(&id);
        }
    }

    Json(json!({ "status": "success" }))
}

pub async fn disable_plugin_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    if state.plugin_manager.is_loaded(&id) {
        if let Err(e) = state.plugin_manager.unload(&id).await {
            return Json(json!({
                "status": "error",
                "message": format!("卸载插件运行时失败: {}", e)
            }));
        }
    }

    if let Err(e) = state.plugins.disable(&id) {
        return Json(json!({ "status": "error", "message": e }));
    }

    state.commands.unregister_plugin_commands(&id);
    Json(json!({ "status": "success" }))
}

pub async fn uninstall_plugin_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let plugin = match state.plugins.get(&id) {
        Some(p) => p,
        None => return Json(json!({ "status": "error", "message": "插件未找到" })),
    };

    if plugin.manifest.builtin {
        return Json(json!({
            "status": "error",
            "message": "内置插件不允许卸载（可在插件中心禁用）"
        }));
    }

    if state.plugin_manager.is_loaded(&id) {
        if let Err(e) = state.plugin_manager.unload(&id).await {
            return Json(json!({
                "status": "error",
                "message": format!("卸载插件运行时失败: {}", e)
            }));
        }
    }

    state.commands.unregister_plugin_commands(&id);

    match state.plugins.uninstall(&id) {
        Ok(_) => Json(json!({ "status": "success" })),
        Err(e) => Json(json!({ "status": "error", "message": e })),
    }
}

pub async fn update_plugin_config_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(config): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let current = match state.plugins.get(&id) {
        Some(p) => p,
        None => return Json(json!({ "status": "error", "message": "插件未找到" })),
    };

    let loaded = state.plugin_manager.is_loaded(&id);
    if loaded {
        if let Err(e) = state
            .plugin_manager
            .update_config(&id, config.clone())
            .await
        {
            return Json(json!({
                "status": "error",
                "message": format!("热更新配置失败: {}", e)
            }));
        }
    }

    if let Err(e) = state.plugins.update_config(&id, config.clone()) {
        if loaded {
            // Keep runtime and persisted state consistent.
            if let Err(e2) = state
                .plugin_manager
                .update_config(&id, current.manifest.config.clone())
                .await
            {
                warn!("插件 {} 配置回滚失败（运行时可能与磁盘不一致）: {}", id, e2);
            }
        }
        return Json(json!({ "status": "error", "message": e }));
    }

    if let Some(plugin) = state.plugins.get(&id) {
        register_plugin_commands(&state.commands, &plugin);
    }

    Json(json!({ "status": "success" }))
}
