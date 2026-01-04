use crate::models::SharedState;
use crate::plugin::InstalledPlugin;
use crate::plugin::types::ConfigSelectOption;
use axum::extract::{Json, Path, State};
use serde_json::json;
use tracing::warn;

use super::commands::register_plugin_commands;

fn get_json_string_by_path(value: &serde_json::Value, path: &str) -> Option<String> {
    let mut cur = value;
    for part in path.split('.') {
        let key = part.trim();
        if key.is_empty() {
            return None;
        }
        cur = cur.get(key)?;
    }
    cur.as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn build_llm_mapping_options(state: &SharedState) -> Vec<ConfigSelectOption> {
    let Some(m) = state.modules.get("llm") else {
        return Vec::new();
    };
    let models = m.config.get("models").and_then(|v| v.as_object());
    let default_alias = m
        .config
        .get("default_model")
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .trim()
        .to_string();

    let mut out: Vec<ConfigSelectOption> = Vec::new();
    if let Some(models) = models {
        let mut keys: Vec<String> = models.keys().cloned().collect();
        keys.sort();
        for alias in keys {
            let label = models
                .get(&alias)
                .and_then(|v| v.as_object())
                .and_then(|o| {
                    let p = o.get("provider").and_then(|v| v.as_str())?.trim();
                    let m = o.get("model").and_then(|v| v.as_str())?.trim();
                    (!p.is_empty() && !m.is_empty()).then(|| format!("{alias} · {p}/{m}"))
                })
                .unwrap_or_else(|| alias.clone());
            out.push(ConfigSelectOption {
                value: alias,
                label,
            });
        }
    }

    if !default_alias.is_empty() && !out.iter().any(|o| o.value == default_alias) {
        out.insert(
            0,
            ConfigSelectOption {
                value: default_alias.clone(),
                label: format!("{default_alias} · (默认)"),
            },
        );
    }

    out
}

fn is_llm_mapping_field(item: &crate::plugin::types::ConfigSchemaItem) -> bool {
    if !item.field_type.eq_ignore_ascii_case("string") {
        return false;
    }
    let key = item.key.to_lowercase();
    if !key.contains("model") {
        return false;
    }
    let desc = item.description.as_deref().unwrap_or("");
    desc.contains("模型") && (desc.contains("映射") || desc.contains("别名") || desc.contains("LLM"))
}

pub async fn list_installed_handler(
    State(state): State<SharedState>,
) -> Json<Vec<InstalledPlugin>> {
    let mut plugins = state.plugins.list();
    let llm_options = build_llm_mapping_options(&state);
    if !llm_options.is_empty() {
        for plugin in plugins.iter_mut() {
            let config = plugin.manifest.config.clone();
            for item in plugin.manifest.config_schema.iter_mut() {
                if !is_llm_mapping_field(item) {
                    continue;
                }

                let mut options = llm_options.clone();

                if let Some(current) = get_json_string_by_path(&config, &item.key) {
                    if !options.iter().any(|o| o.value == current) {
                        options.push(ConfigSelectOption {
                            value: current.clone(),
                            label: format!("{current} · (未映射)"),
                        });
                    }
                }
                if let Some(def) = item
                    .default
                    .as_ref()
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                {
                    if !options.iter().any(|o| o.value == def) {
                        options.push(ConfigSelectOption {
                            value: def.clone(),
                            label: format!("{def} · (默认值)"),
                        });
                    }
                }

                item.field_type = "select".to_string();
                item.options = Some(options);
            }
        }
    }
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
