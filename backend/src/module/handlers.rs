use super::BotModule;
use crate::models::SharedState;
use axum::extract::{Json, Path, State};
use serde_json::json;

pub async fn list_modules_handler(State(state): State<SharedState>) -> Json<Vec<BotModule>> {
    Json(state.modules.list())
}

pub async fn get_module_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Json<BotModule> {
    state
        .modules
        .get(&id)
        .map(Json)
        .unwrap_or_else(|| Json(BotModule::default()))
}

pub async fn enable_module_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    match state.modules.enable(&id) {
        Ok(_) => Json(serde_json::json!({ "status": "success" })),
        Err(e) => Json(serde_json::json!({ "status": "error", "message": e })),
    }
}

pub async fn disable_module_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    match state.modules.disable(&id) {
        Ok(_) => Json(serde_json::json!({ "status": "success" })),
        Err(e) => Json(serde_json::json!({ "status": "error", "message": e })),
    }
}

#[derive(serde::Deserialize)]
pub struct UpdateConfigPayload {
    pub config: serde_json::Value,
}

pub async fn update_module_config_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateConfigPayload>,
) -> Json<serde_json::Value> {
    match state.modules.update_config(&id, payload.config) {
        Ok(_) => Json(serde_json::json!({ "status": "success" })),
        Err(e) => Json(serde_json::json!({ "status": "error", "message": e })),
    }
}

// LLM specific handlers

#[derive(serde::Deserialize)]
pub struct LLMTestPayload {
    pub provider: String,
    pub api_key: String,
    #[serde(default)]
    pub base_url: String,
}

pub async fn llm_test_handler(Json(payload): Json<LLMTestPayload>) -> Json<serde_json::Value> {
    let client = reqwest::Client::new();

    // Use /models endpoint for health check instead of chat completion
    let req = match payload.provider.as_str() {
        "claude" => {
            // Anthropic doesn't have a models endpoint, just verify the API key format
            if payload.api_key.is_empty() {
                return Json(json!({ "status": "error", "message": "API Key 不能为空" }));
            }
            if !payload.api_key.starts_with("sk-ant-") {
                return Json(
                    json!({ "status": "error", "message": "Claude API Key 格式不正确，应以 sk-ant- 开头" }),
                );
            }
            return Json(json!({ "status": "success", "message": "API Key 格式正确" }));
        }
        _ => {
            let base = if payload.base_url.is_empty() {
                "https://api.openai.com/v1"
            } else {
                payload.base_url.trim_end_matches('/')
            };
            let url = format!("{}/models", base);
            client
                .get(&url)
                .header("Authorization", format!("Bearer {}", payload.api_key))
        }
    };

    match req.send().await {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                Json(json!({ "status": "success", "message": "连接成功" }))
            } else {
                let text = match resp.text().await {
                    Ok(t) => t,
                    Err(e) => format!("(无法读取响应正文: {e})"),
                };
                Json(json!({ "status": "error", "message": format!("HTTP {}: {}", status, text) }))
            }
        }
        Err(e) => Json(json!({ "status": "error", "message": e.to_string() })),
    }
}

#[derive(serde::Deserialize)]
pub struct LLMModelsPayload {
    pub provider: String,
    pub api_key: String,
    #[serde(default)]
    pub base_url: String,
}

pub async fn llm_models_handler(Json(payload): Json<LLMModelsPayload>) -> Json<serde_json::Value> {
    let client = reqwest::Client::new();

    match payload.provider.as_str() {
        "claude" => {
            // Anthropic doesn't have a models endpoint, return hardcoded list
            Json(json!({
                "status": "success",
                "models": [
                    "claude-3-5-sonnet-20241022",
                    "claude-3-5-haiku-20241022",
                    "claude-3-opus-20240229",
                    "claude-3-sonnet-20240229",
                    "claude-3-haiku-20240307"
                ]
            }))
        }
        _ => {
            let base = if payload.base_url.is_empty() {
                "https://api.openai.com/v1"
            } else {
                payload.base_url.trim_end_matches('/')
            };
            let url = format!("{}/models", base);

            match client
                .get(&url)
                .header("Authorization", format!("Bearer {}", payload.api_key))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(data) = resp.json::<serde_json::Value>().await {
                        let Some(arr) = data.get("data").and_then(|v| v.as_array()) else {
                            return Json(
                                json!({ "status": "error", "message": "响应格式错误：缺少 data 数组" }),
                            );
                        };
                        let mut models: Vec<String> = arr
                            .iter()
                            .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(String::from))
                            .collect();
                        // Sort: chat models first, then alphabetically
                        models.sort_by(|a, b| {
                            let a_chat = a.contains("gpt")
                                || a.contains("chat")
                                || a.contains("o1")
                                || a.contains("o3");
                            let b_chat = b.contains("gpt")
                                || b.contains("chat")
                                || b.contains("o1")
                                || b.contains("o3");
                            match (a_chat, b_chat) {
                                (true, false) => std::cmp::Ordering::Less,
                                (false, true) => std::cmp::Ordering::Greater,
                                _ => a.cmp(b),
                            }
                        });
                        Json(json!({ "status": "success", "models": models }))
                    } else {
                        Json(json!({ "status": "error", "message": "解析响应失败" }))
                    }
                }
                Ok(resp) => {
                    let text = match resp.text().await {
                        Ok(t) => t,
                        Err(e) => format!("(无法读取响应正文: {e})"),
                    };
                    Json(json!({ "status": "error", "message": text }))
                }
                Err(e) => Json(json!({ "status": "error", "message": e.to_string() })),
            }
        }
    }
}

/// Get LLM configuration (from llm module config)
pub async fn get_llm_config_handler(State(state): State<SharedState>) -> Json<serde_json::Value> {
    let module = state.modules.get("llm");
    match module {
        Some(m) => {
            let config = &m.config;
            Json(json!({
                "status": "success",
                "providers": config.get("providers").cloned().unwrap_or(json!([])),
                "model_library": config.get("model_library").cloned().unwrap_or(json!([])),
                "mappings": config.get("models").cloned().unwrap_or(json!({})),
                "default_model": config.get("default_model").and_then(|v| v.as_str()).unwrap_or("default"),
                "tavily_api_key": config.get("tavily_api_key").and_then(|v| v.as_str()).unwrap_or("")
            }))
        }
        None => Json(json!({
            "status": "success",
            "providers": [],
            "model_library": [],
            "mappings": {},
            "default_model": "default",
            "tavily_api_key": ""
        })),
    }
}

#[derive(serde::Deserialize)]
pub struct UpdateLLMConfigPayload {
    pub providers: serde_json::Value,
    #[serde(default)]
    pub model_library: serde_json::Value,
    pub mappings: serde_json::Value,
    pub default_model: String,
    #[serde(default)]
    pub tavily_api_key: String,
}

/// Update LLM configuration
pub async fn update_llm_config_handler(
    State(state): State<SharedState>,
    Json(payload): Json<UpdateLLMConfigPayload>,
) -> Json<serde_json::Value> {
    let new_config = json!({
        "providers": payload.providers,
        "model_library": payload.model_library,
        "models": payload.mappings,
        "default_model": payload.default_model,
        "tavily_api_key": payload.tavily_api_key
    });

    match state.modules.update_config("llm", new_config) {
        Ok(_) => Json(json!({ "status": "success" })),
        Err(e) => Json(json!({ "status": "error", "message": e })),
    }
}

#[derive(serde::Deserialize)]
pub struct TavilyTestPayload {
    pub api_key: String,
}

/// Test Tavily API key
pub async fn tavily_test_handler(Json(payload): Json<TavilyTestPayload>) -> Json<serde_json::Value> {
    if payload.api_key.is_empty() {
        return Json(json!({ "status": "error", "message": "API Key 不能为空" }));
    }

    // Tavily API keys start with "tvly-"
    if !payload.api_key.starts_with("tvly-") {
        return Json(json!({ "status": "error", "message": "Tavily API Key 格式不正确，应以 tvly- 开头" }));
    }

    let client = reqwest::Client::new();
    let body = json!({
        "api_key": payload.api_key,
        "query": "test",
        "max_results": 1
    });

    match client
        .post("https://api.tavily.com/search")
        .json(&body)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                Json(json!({ "status": "success", "message": "Tavily API 连接成功" }))
            } else {
                let text = match resp.text().await {
                    Ok(t) => t,
                    Err(e) => format!("(无法读取响应正文: {e})"),
                };
                Json(json!({ "status": "error", "message": format!("HTTP {}: {}", status, text) }))
            }
        }
        Err(e) => Json(json!({ "status": "error", "message": e.to_string() })),
    }
}

#[derive(serde::Deserialize)]
pub struct LLMChatPayload {
    pub provider: String,
    pub api_key: String,
    #[serde(default)]
    pub base_url: String,
    pub model: String,
    pub messages: Vec<serde_json::Value>,
}

/// Chat with LLM for testing
pub async fn llm_chat_handler(Json(payload): Json<LLMChatPayload>) -> Json<serde_json::Value> {
    if payload.api_key.is_empty() {
        return Json(json!({ "status": "error", "message": "API Key 不能为空" }));
    }
    if payload.model.is_empty() {
        return Json(json!({ "status": "error", "message": "模型不能为空" }));
    }
    if payload.messages.is_empty() {
        return Json(json!({ "status": "error", "message": "消息不能为空" }));
    }

    let client = reqwest::Client::new();

    match payload.provider.as_str() {
        "claude" => {
            // Anthropic API
            let base = if payload.base_url.is_empty() {
                "https://api.anthropic.com"
            } else {
                payload.base_url.trim_end_matches('/')
            };
            let url = format!("{}/v1/messages", base);

            // Convert messages format for Claude
            let mut system_prompt = String::new();
            let mut claude_messages: Vec<serde_json::Value> = Vec::new();

            for msg in &payload.messages {
                let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
                let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");

                if role == "system" {
                    system_prompt = content.to_string();
                } else {
                    claude_messages.push(json!({
                        "role": role,
                        "content": content
                    }));
                }
            }

            let mut body = json!({
                "model": payload.model,
                "max_tokens": 4096,
                "messages": claude_messages
            });

            if !system_prompt.is_empty() {
                body["system"] = json!(system_prompt);
            }

            match client
                .post(&url)
                .header("x-api-key", &payload.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("Content-Type", "application/json")
                .json(&body)
                .timeout(std::time::Duration::from_secs(120))
                .send()
                .await
            {
                Ok(resp) => {
                    let status = resp.status();
                    let text = match resp.text().await {
                        Ok(t) => t,
                        Err(e) => return Json(json!({ "status": "error", "message": format!("读取响应失败: {e}") })),
                    };

                    if status.is_success() {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                            let content = v.get("content")
                                .and_then(|c| c.get(0))
                                .and_then(|c| c.get("text"))
                                .and_then(|t| t.as_str())
                                .unwrap_or("");
                            Json(json!({ "status": "success", "content": content }))
                        } else {
                            Json(json!({ "status": "error", "message": "解析响应失败" }))
                        }
                    } else {
                        let msg = serde_json::from_str::<serde_json::Value>(&text)
                            .ok()
                            .and_then(|v| v.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()).map(|s| s.to_string()))
                            .unwrap_or_else(|| text.chars().take(200).collect());
                        Json(json!({ "status": "error", "message": format!("HTTP {}: {}", status, msg) }))
                    }
                }
                Err(e) => Json(json!({ "status": "error", "message": e.to_string() })),
            }
        }
        _ => {
            // OpenAI compatible API
            let base = if payload.base_url.is_empty() {
                "https://api.openai.com/v1"
            } else {
                payload.base_url.trim_end_matches('/')
            };
            let url = format!("{}/chat/completions", base);

            let body = json!({
                "model": payload.model,
                "messages": payload.messages,
                "max_tokens": 4096
            });

            match client
                .post(&url)
                .header("Authorization", format!("Bearer {}", payload.api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .timeout(std::time::Duration::from_secs(120))
                .send()
                .await
            {
                Ok(resp) => {
                    let status = resp.status();
                    let text = match resp.text().await {
                        Ok(t) => t,
                        Err(e) => return Json(json!({ "status": "error", "message": format!("读取响应失败: {e}") })),
                    };

                    if status.is_success() {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                            let content = v.get("choices")
                                .and_then(|c| c.get(0))
                                .and_then(|c| c.get("message"))
                                .and_then(|m| m.get("content"))
                                .and_then(|c| c.as_str())
                                .unwrap_or("");
                            Json(json!({ "status": "success", "content": content }))
                        } else {
                            Json(json!({ "status": "error", "message": "解析响应失败" }))
                        }
                    } else {
                        let msg = serde_json::from_str::<serde_json::Value>(&text)
                            .ok()
                            .and_then(|v| v.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()).map(|s| s.to_string()))
                            .unwrap_or_else(|| text.chars().take(200).collect());
                        Json(json!({ "status": "error", "message": format!("HTTP {}: {}", status, msg) }))
                    }
                }
                Err(e) => Json(json!({ "status": "error", "message": e.to_string() })),
            }
        }
    }
}
