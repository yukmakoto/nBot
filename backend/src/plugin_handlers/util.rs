use axum::Json;
use serde_json::json;

pub(super) fn allow_unsigned_plugins() -> bool {
    matches!(
        std::env::var("NBOT_ALLOW_UNSIGNED_PLUGINS")
            .unwrap_or_else(|_| "false".to_string())
            .trim()
            .to_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on"
    )
}

pub(super) fn is_safe_path_segment(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() || s.len() > 64 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

pub(super) fn json_success(plugin_id: Option<&str>) -> Json<serde_json::Value> {
    let mut v = json!({ "status": "success" });
    if let Some(id) = plugin_id {
        v["plugin_id"] = json!(id);
    }
    Json(v)
}

pub(super) fn json_error(message: impl ToString) -> Json<serde_json::Value> {
    Json(json!({ "status": "error", "message": message.to_string() }))
}
