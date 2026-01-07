use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use futures_util::StreamExt;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::bot::runtime::api::send_reply;
use crate::bot::runtime::BotRuntime;
use crate::models::SharedState;

use super::super::download::TempFileGuard;

mod forward;

pub(in super::super) use forward::{send_llm_markdown_as_forward_image, SendForwardImageInput};

#[derive(Debug, Clone)]
pub(in super::super::super) struct LlmConfig {
    pub(in super::super::super) base_url: String,
    pub(in super::super::super) api_key: String,
    pub(in super::super::super) model_name: String,
    pub(in super::super::super) max_request_bytes: u64,
}

#[derive(Debug, Clone)]
pub(in super::super::super) enum LlmCallError {
    RequestTooLarge {
        request_bytes: u64,
        limit_bytes: u64,
    },
    Http {
        status: u16,
        message: String,
    },
    Transport(String),
    Decode(String),
    Parse(String),
    MissingContent,
}

impl LlmCallError {
    pub(super) fn http_status(&self) -> Option<u16> {
        match self {
            LlmCallError::Http { status, .. } => Some(*status),
            _ => None,
        }
    }
}

impl std::fmt::Display for LlmCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmCallError::RequestTooLarge {
                request_bytes,
                limit_bytes,
            } => write!(
                f,
                "请求体过大：{} bytes，超过限制 {} bytes",
                request_bytes, limit_bytes
            ),
            LlmCallError::Http { status, message } => {
                write!(f, "LLM API 返回错误 (HTTP {}): {}", status, message)
            }
            LlmCallError::Transport(e) => write!(f, "LLM API 请求失败: {}", e),
            LlmCallError::Decode(e) => write!(f, "读取 LLM 响应失败: {}", e),
            LlmCallError::Parse(e) => write!(f, "解析 LLM 响应失败: {}", e),
            LlmCallError::MissingContent => write!(f, "分析失败：无法获取回复内容"),
        }
    }
}

fn parse_retry_after_seconds_from_message(message: &str) -> Option<u64> {
    let lower = message.to_lowercase();
    let after_idx = lower.find("after")?;
    let tail = &lower[after_idx + "after".len()..];
    let digits: String = tail
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse::<u64>().ok()
}

fn parse_retry_after_seconds_from_headers(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
}

fn should_retry_llm_http_status(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504)
}

#[derive(Debug, Clone)]
pub(in super::super) struct BinaryMeta {
    pub(in super::super) file_name: Option<String>,
    pub(in super::super) file_ext: Option<String>,
    pub(in super::super) size_bytes: u64,
    pub(in super::super) truncated: bool,
}

pub(in super::super) async fn download_binary_to_temp(
    url: &str,
    file_name: Option<&str>,
    timeout_ms: u64,
    max_bytes: u64,
) -> Result<(TempFileGuard, BinaryMeta), String> {
    let safe_name = file_name.unwrap_or("download.bin");
    let guard = TempFileGuard::new("download", Some(safe_name)).await?;

    let timeout = std::time::Duration::from_millis(timeout_ms.clamp(1000, 120000));
    let max_bytes = max_bytes.clamp(10_000, 200_000_000);

    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| format!("Download failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("Download failed: HTTP {}", resp.status()));
    }

    let mut file = tokio::fs::File::create(&guard.path)
        .await
        .map_err(|e| format!("Create temp file failed: {e}"))?;

    let mut downloaded: u64 = 0;
    let mut truncated_by_bytes = false;

    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Read download stream failed: {e}"))?;
        if downloaded >= max_bytes {
            truncated_by_bytes = true;
            break;
        }

        let remaining = (max_bytes - downloaded) as usize;
        let slice: &[u8] = if chunk.len() > remaining {
            truncated_by_bytes = true;
            &chunk[..remaining]
        } else {
            &chunk
        };

        file.write_all(slice)
            .await
            .map_err(|e| format!("Write temp file failed: {e}"))?;
        downloaded += slice.len() as u64;

        if truncated_by_bytes {
            break;
        }
    }

    let meta = BinaryMeta {
        file_name: file_name.map(|s| s.to_string()),
        file_ext: file_name
            .and_then(|s| Path::new(s).extension().and_then(|e| e.to_str()))
            .map(|s| s.to_lowercase()),
        size_bytes: downloaded,
        truncated: truncated_by_bytes,
    };

    Ok((guard, meta))
}

/// Download a remote image and convert it into a compact `data:` URL for multimodal chat.
/// This is used by plugin `callLlmChat` so text-only plugins can still attach images reliably
/// without depending on the provider to fetch remote URLs.
#[allow(dead_code)]
pub(in super::super::super) async fn download_and_prepare_image_data_url(
    url: &str,
    timeout_ms: u64,
    max_bytes: u64,
    max_width: u32,
    max_height: u32,
    jpeg_quality: u8,
    max_output_bytes: u64,
) -> Result<String, String> {
    let (guard, _meta) = download_binary_to_temp(url, None, timeout_ms, max_bytes).await?;
    let (data_url, _prepared) = super::image::prepare_image_data_url(
        &guard.path,
        max_width,
        max_height,
        jpeg_quality,
        max_output_bytes,
    )
    .await?;
    Ok(data_url)
}

fn guess_file_name_from_url(url: &str) -> Option<String> {
    let u = url.trim();
    if u.is_empty() {
        return None;
    }
    let without_query = u.split('?').next().unwrap_or(u);
    let last = without_query
        .rsplit('/')
        .next()
        .unwrap_or(without_query)
        .trim();
    if last.is_empty() {
        None
    } else {
        Some(last.to_string())
    }
}

fn sniff_binary_kind_prefix(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 12 && bytes[0..4] == *b"RIFF" && bytes[8..12] == *b"WAVE" {
        return Some("wav");
    }
    if bytes.len() >= 4 && bytes[0..4] == [0x1A, 0x45, 0xDF, 0xA3] {
        return Some("webm");
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Some("mp4");
    }
    if bytes.len() >= 4 && bytes[0..4] == *b"OggS" {
        return Some("ogg");
    }
    if bytes.len() >= 4 && bytes[0..4] == *b"fLaC" {
        return Some("flac");
    }
    if bytes.len() >= 3 && bytes[0..3] == *b"ID3" {
        return Some("mp3");
    }
    if bytes.len() >= 2 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0 {
        return Some("mp3");
    }
    None
}

async fn sniff_file_ext_from_path(path: &Path) -> Option<&'static str> {
    use tokio::io::AsyncReadExt;
    let mut f = tokio::fs::File::open(path).await.ok()?;
    let mut buf = [0u8; 32];
    let n = f.read(&mut buf).await.ok()?;
    sniff_binary_kind_prefix(&buf[..n])
}

/// Best-effort: download a remote media URL and convert it to a `data:` URL suitable for multimodal chat.
/// - If the file looks like an image, it will be re-encoded/compressed (same as `download_and_prepare_image_data_url`).
/// - Otherwise, it will be embedded as-is via base64 (`data:video/*` or `data:audio/*` or octet-stream).
pub(in super::super::super) async fn download_and_prepare_media_data_url(
    url: &str,
    timeout_ms: u64,
    max_bytes: u64,
    image_max_width: u32,
    image_max_height: u32,
    image_jpeg_quality: u8,
    image_max_output_bytes: u64,
) -> Result<String, String> {
    let guess_name = guess_file_name_from_url(url);
    let (guard, meta) =
        download_binary_to_temp(url, guess_name.as_deref(), timeout_ms, max_bytes).await?;

    // Try to treat it as an image first (fast path for screenshots).
    if let Ok((data_url, _prepared)) = super::image::prepare_image_data_url(
        &guard.path,
        image_max_width,
        image_max_height,
        image_jpeg_quality,
        image_max_output_bytes,
    )
    .await
    {
        return Ok(data_url);
    }

    let sniff_ext = sniff_file_ext_from_path(&guard.path).await;
    let name = meta
        .file_name
        .clone()
        .or_else(|| guess_name)
        .unwrap_or_else(|| "media.bin".to_string());
    let ext = meta
        .file_ext
        .clone()
        .or_else(|| sniff_ext.map(|s| s.to_string()));

    let mime_name = match ext.as_deref() {
        Some("mp4") => "video.mp4".to_string(),
        Some("webm") => "video.webm".to_string(),
        Some("wav") => "audio.wav".to_string(),
        Some("mp3") => "audio.mp3".to_string(),
        Some("m4a") => "audio.m4a".to_string(),
        Some("ogg") => "audio.ogg".to_string(),
        Some("flac") => "audio.flac".to_string(),
        Some(other) if !other.is_empty() => format!("file.{}", other),
        _ => name,
    };

    read_file_as_data_url(&guard.path, &mime_name).await
}

pub(super) async fn get_record_base64_as_temp(
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    record_file: &str,
    file_name: Option<&str>,
    max_bytes: u64,
) -> Result<(TempFileGuard, BinaryMeta), String> {
    let resp = runtime
        .call_api(
            bot_id,
            "get_record",
            json!({
                "file": record_file,
                "out_format": "wav",
            }),
        )
        .await
        .ok_or_else(|| "调用 get_record 失败：无响应".to_string())?;

    let data = resp.get("data").unwrap_or(&resp);
    let base64_str = data
        .get("base64")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "get_record 失败：响应缺少 base64 字段".to_string())?;

    let b64 = base64_str
        .trim()
        .strip_prefix("base64://")
        .unwrap_or(base64_str);
    let bytes = BASE64
        .decode(b64.as_bytes())
        .map_err(|e| format!("解析语音 base64 失败: {e}"))?;

    let max_bytes = max_bytes.clamp(10_000, 200_000_000);
    if (bytes.len() as u64) > max_bytes {
        return Err(format!(
            "语音过大：{} bytes，超过限制 {} bytes",
            bytes.len(),
            max_bytes
        ));
    }

    let safe_name = file_name.unwrap_or("record.wav");
    let guard = TempFileGuard::new("record", Some(safe_name)).await?;

    let mut file = tokio::fs::File::create(&guard.path)
        .await
        .map_err(|e| format!("Create temp file failed: {e}"))?;
    file.write_all(&bytes)
        .await
        .map_err(|e| format!("Write temp file failed: {e}"))?;

    let meta = BinaryMeta {
        file_name: file_name.map(|s| s.to_string()),
        file_ext: file_name
            .and_then(|s| Path::new(s).extension().and_then(|e| e.to_str()))
            .map(|s| s.to_lowercase()),
        size_bytes: bytes.len() as u64,
        truncated: false,
    };

    Ok((guard, meta))
}

pub(super) async fn read_file_as_data_url(
    file_path: &Path,
    file_name: &str,
) -> Result<String, String> {
    let bytes = tokio::fs::read(file_path)
        .await
        .map_err(|e| format!("读取文件失败: {e}"))?;
    let mime = guess_transcription_mime(file_name);
    let b64 = BASE64.encode(&bytes);
    Ok(format!("data:{mime};base64,{b64}"))
}

#[allow(dead_code)]
pub(in super::super::super) fn resolve_llm_config(
    state: &SharedState,
    bot_id: &str,
) -> Result<LlmConfig, String> {
    resolve_llm_config_by_name(state, bot_id, None)
}

/// 根据模型映射名称解析 LLM 配置
/// 如果 model_mapping_name 为 None，则使用默认模型
pub(in super::super::super) fn resolve_llm_config_by_name(
    state: &SharedState,
    bot_id: &str,
    model_mapping_name: Option<&str>,
) -> Result<LlmConfig, String> {
    let llm_module = match crate::module::get_effective_module(state, bot_id, "llm") {
        Some(m) if m.enabled => m,
        _ => return Err("LLM 模块未启用，请先在设置中启用并配置 LLM 模块".to_string()),
    };

    let providers = llm_module
        .config
        .get("providers")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "LLM 模块配置错误：providers 缺失或格式不正确".to_string())?;

    let models = llm_module
        .config
        .get("models")
        .and_then(|v| v.as_object())
        .ok_or_else(|| "LLM 模块配置错误：models 缺失或格式不正确".to_string())?;

    // 确定要使用的模型映射名称
    let target_model_name = match model_mapping_name {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => llm_module
            .config
            .get("default_model")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "LLM 模块配置错误：default_model 缺失".to_string())?
            .to_string(),
    };

    let model_config = models
        .get(&target_model_name)
        .ok_or_else(|| format!("LLM 模块配置错误：未找到模型映射 '{}'", target_model_name))?;

    let provider_id = model_config
        .get("provider")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let model_name = model_config
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if provider_id.is_empty() || model_name.is_empty() {
        return Err(format!(
            "LLM 模块配置错误：模型 '{}' 字段不完整",
            target_model_name
        ));
    }

    let provider = providers
        .iter()
        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(provider_id))
        .ok_or_else(|| format!("LLM 模块配置错误：未找到提供商 '{}'", provider_id))?;

    let api_key = provider
        .get("api_key")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if api_key.is_empty() {
        return Err("LLM 模块配置错误：API Key 未设置".to_string());
    }

    let base_url = provider
        .get("base_url")
        .and_then(|v| v.as_str())
        .unwrap_or("https://api.openai.com/v1")
        .to_string();
    let max_request_bytes = provider
        .get("max_request_bytes")
        .and_then(|v| v.as_u64())
        .unwrap_or(4_000_000)
        .clamp(200_000, 200_000_000);

    Ok(LlmConfig {
        base_url,
        api_key,
        model_name: model_name.to_string(),
        max_request_bytes,
    })
}

pub(in super::super::super) async fn call_chat_completions(
    base_url: &str,
    api_key: &str,
    request_body: &serde_json::Value,
    max_request_bytes: u64,
) -> Result<String, LlmCallError> {
    fn mask_long_digits_for_log(input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let mut run = String::new();
        for ch in input.chars() {
            if ch.is_ascii_digit() {
                run.push(ch);
                continue;
            }
            if !run.is_empty() {
                if run.len() >= 5 {
                    out.push_str("***");
                } else {
                    out.push_str(&run);
                }
                run.clear();
            }
            out.push(ch);
        }
        if !run.is_empty() {
            if run.len() >= 5 {
                out.push_str("***");
            } else {
                out.push_str(&run);
            }
        }
        out
    }

    fn compact_for_log(input: &str, max_len: usize) -> String {
        let s = input
            .replace('\r', " ")
            .replace('\n', " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let masked = mask_long_digits_for_log(&s);
        if masked.len() <= max_len {
            return masked;
        }
        masked.chars().take(max_len).collect::<String>() + "..."
    }

    fn extract_content_from_choice(choice: &serde_json::Value) -> Option<String> {
        let message = choice.get("message")?;
        let content = message.get("content")?;
        match content {
            serde_json::Value::String(s) => Some(s.to_string()),
            serde_json::Value::Array(parts) => {
                let mut out = String::new();
                for part in parts {
                    match part {
                        serde_json::Value::String(s) => out.push_str(s),
                        serde_json::Value::Object(map) => {
                            if let Some(t) = map.get("text").and_then(|v| v.as_str()) {
                                out.push_str(t);
                            } else if let Some(t) = map.get("content").and_then(|v| v.as_str()) {
                                out.push_str(t);
                            } else if let Some(t) = map.get("value").and_then(|v| v.as_str()) {
                                out.push_str(t);
                            }
                        }
                        _ => {}
                    }
                }
                if out.trim().is_empty() {
                    None
                } else {
                    Some(out)
                }
            }
            _ => None,
        }
    }

    fn extract_chat_content(v: &serde_json::Value) -> Option<String> {
        // OpenAI Chat Completions format
        if let Some(choice) = v.get("choices").and_then(|c| c.get(0)) {
            if let Some(content) = extract_content_from_choice(choice) {
                return Some(content);
            }
            // Legacy (some gateways still return choices[0].text)
            if let Some(text) = choice.get("text").and_then(|t| t.as_str()) {
                if !text.trim().is_empty() {
                    return Some(text.to_string());
                }
            }
        }

        // OpenAI Responses API-like format fallback
        if let Some(output) = v.get("output").and_then(|o| o.get(0)) {
            if let Some(content) = output
                .get("content")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("text"))
                .and_then(|t| t.as_str())
            {
                if !content.trim().is_empty() {
                    return Some(content.to_string());
                }
            }
        }
        if let Some(text) = v.get("output_text").and_then(|t| t.as_str()) {
            if !text.trim().is_empty() {
                return Some(text.to_string());
            }
        }

        None
    }

    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let request_bytes = serde_json::to_vec(request_body)
        .map_err(|e| LlmCallError::Parse(format!("序列化请求失败: {e}")))?;
    if (request_bytes.len() as u64) > max_request_bytes {
        return Err(LlmCallError::RequestTooLarge {
            request_bytes: request_bytes.len() as u64,
            limit_bytes: max_request_bytes,
        });
    }

    let timeout = std::time::Duration::from_secs(180);
    let max_attempts: usize = 3;

    for attempt in 0..max_attempts {
        let resp = match client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .body(request_bytes.clone())
            .timeout(timeout)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                let err = LlmCallError::Transport(e.to_string());
                if attempt + 1 >= max_attempts {
                    return Err(err);
                }
                let delay_ms = 300_u64
                    .saturating_mul(2_u64.saturating_pow(attempt as u32))
                    .min(3000);
                warn!("LLM request failed, retrying in {}ms: {}", delay_ms, err);
                sleep(std::time::Duration::from_millis(delay_ms)).await;
                continue;
            }
        };

        let status = resp.status();
        let headers = resp.headers().clone();
        let text = resp
            .text()
            .await
            .map_err(|e| LlmCallError::Decode(e.to_string()))?;

        if !status.is_success() {
            let msg = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| {
                    v.get("error")
                        .cloned()
                        .or_else(|| v.get("message").cloned())
                })
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| text.chars().take(400).collect());

            let http_status = status.as_u16();
            let err = LlmCallError::Http {
                status: http_status,
                message: msg.clone(),
            };

            if attempt + 1 < max_attempts && should_retry_llm_http_status(http_status) {
                let mut delay_ms = 500_u64
                    .saturating_mul(2_u64.saturating_pow(attempt as u32))
                    .min(5000);
                if http_status == 429 {
                    let mut delay_secs = parse_retry_after_seconds_from_headers(&headers)
                        .or_else(|| parse_retry_after_seconds_from_message(&msg))
                        .unwrap_or(1);
                    delay_secs = delay_secs.clamp(1, 60);
                    delay_ms = delay_secs.saturating_mul(1000);
                }
                warn!(
                    "LLM HTTP {}, retrying in {}ms: {}",
                    http_status,
                    delay_ms,
                    compact_for_log(&msg, 140)
                );
                sleep(std::time::Duration::from_millis(delay_ms)).await;
                continue;
            }

            return Err(err);
        }

        let v: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| LlmCallError::Parse(e.to_string()))?;
        let content = extract_chat_content(&v).ok_or(LlmCallError::MissingContent)?;

        // Debug suspiciously short outputs: print a compact preview of the raw response (redacted).
        let trimmed = content.trim();
        if trimmed.chars().count() <= 6 && text.len() > 200 {
            let finish_reason = v
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("finish_reason"))
                .and_then(|f| f.as_str())
                .unwrap_or("");
            warn!(
                "LLM short content (len={} finish_reason={}): raw={}",
                trimmed.chars().count(),
                finish_reason,
                compact_for_log(&text, 700)
            );
        }

        return Ok(content);
    }

    Err(LlmCallError::Transport("LLM 重试失败".to_string()))
}

/// Tavily 搜索工具定义
fn tavily_tool_definition() -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": "tavily_search",
            "description": "Search the web for current information using Tavily. Use this when you need to find up-to-date information, facts, news, or any information that might have changed after your knowledge cutoff.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query to look up on the web"
                    }
                },
                "required": ["query"]
            }
        }
    })
}

/// 调用 Tavily 搜索 API
async fn call_tavily_search(tavily_api_key: &str, query: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.tavily.com/search")
        .header("Content-Type", "application/json")
        .json(&json!({
            "api_key": tavily_api_key,
            "query": query,
            "search_depth": "basic",
            "include_answer": true,
            "include_raw_content": false,
            "max_results": 5
        }))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("Tavily 请求失败: {e}"))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取 Tavily 响应失败: {e}"))?;

    if !status.is_success() {
        return Err(format!("Tavily API 错误 (HTTP {}): {}", status, text));
    }

    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("解析 Tavily 响应失败: {e}"))?;

    // 构建搜索结果摘要
    let mut result = String::new();

    // 如果有 answer，优先使用
    if let Some(answer) = v.get("answer").and_then(|a| a.as_str()) {
        result.push_str("## 搜索摘要\n");
        result.push_str(answer);
        result.push_str("\n\n");
    }

    // 添加搜索结果
    if let Some(results) = v.get("results").and_then(|r| r.as_array()) {
        result.push_str("## 搜索结果\n\n");
        for (i, item) in results.iter().take(5).enumerate() {
            let title = item
                .get("title")
                .and_then(|t| t.as_str())
                .unwrap_or("无标题");
            let url = item.get("url").and_then(|u| u.as_str()).unwrap_or("");
            let content = item
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or("无内容");

            result.push_str(&format!("### {}. {}\n", i + 1, title));
            result.push_str(&format!("链接: {}\n", url));
            result.push_str(&format!("{}\n\n", content));
        }
    }

    if result.is_empty() {
        result = "未找到相关搜索结果".to_string();
    }

    Ok(result)
}

/// 调用支持 Tavily 搜索的 LLM
/// 如果提供了 tavily_api_key，则使用函数调用模式
/// 否则回退到简单的搜索参数模式
pub(in super::super::super) async fn call_chat_completions_with_tavily(
    base_url: &str,
    api_key: &str,
    request_body: &serde_json::Value,
    max_request_bytes: u64,
    enable_search: bool,
    tavily_api_key: Option<&str>,
) -> Result<String, LlmCallError> {
    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    // 如果有 Tavily API key 且启用搜索，使用函数调用模式
    if enable_search && tavily_api_key.is_some() && !tavily_api_key.unwrap().is_empty() {
        return call_with_tavily_tool_loop(
            &client,
            &url,
            api_key,
            request_body,
            max_request_bytes,
            tavily_api_key.unwrap(),
        )
        .await;
    }

    // 否则使用简单搜索参数模式
    let mut body = request_body.clone();
    if enable_search {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("web_search".to_string(), json!(true));
            obj.insert("search".to_string(), json!(true));
            obj.insert("online".to_string(), json!(true));
        }
    }

    let request_bytes = serde_json::to_vec(&body)
        .map_err(|e| LlmCallError::Parse(format!("序列化请求失败: {e}")))?;
    if (request_bytes.len() as u64) > max_request_bytes {
        return Err(LlmCallError::RequestTooLarge {
            request_bytes: request_bytes.len() as u64,
            limit_bytes: max_request_bytes,
        });
    }

    let timeout = std::time::Duration::from_secs(300);
    let max_attempts: usize = 3;

    for attempt in 0..max_attempts {
        let resp = match client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .body(request_bytes.clone())
            .timeout(timeout)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                let err = LlmCallError::Transport(e.to_string());
                if attempt + 1 >= max_attempts {
                    return Err(err);
                }
                let delay_ms = 300_u64
                    .saturating_mul(2_u64.saturating_pow(attempt as u32))
                    .min(3000);
                warn!("LLM request failed, retrying in {}ms: {}", delay_ms, err);
                sleep(std::time::Duration::from_millis(delay_ms)).await;
                continue;
            }
        };

        let status = resp.status();
        let headers = resp.headers().clone();
        let text = resp
            .text()
            .await
            .map_err(|e| LlmCallError::Decode(e.to_string()))?;

        if !status.is_success() {
            let msg = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| {
                    v.get("error")
                        .cloned()
                        .or_else(|| v.get("message").cloned())
                })
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| text.chars().take(400).collect());

            let http_status = status.as_u16();
            let err = LlmCallError::Http {
                status: http_status,
                message: msg.clone(),
            };

            if attempt + 1 < max_attempts && should_retry_llm_http_status(http_status) {
                let mut delay_ms = 500_u64
                    .saturating_mul(2_u64.saturating_pow(attempt as u32))
                    .min(5000);
                if http_status == 429 {
                    let mut delay_secs = parse_retry_after_seconds_from_headers(&headers)
                        .or_else(|| parse_retry_after_seconds_from_message(&msg))
                        .unwrap_or(1);
                    delay_secs = delay_secs.clamp(1, 60);
                    delay_ms = delay_secs.saturating_mul(1000);
                }
                warn!(
                    "LLM HTTP {}, retrying in {}ms: {}",
                    http_status,
                    delay_ms,
                    msg.chars().take(140).collect::<String>()
                );
                sleep(std::time::Duration::from_millis(delay_ms)).await;
                continue;
            }

            return Err(err);
        }

        let v: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| LlmCallError::Parse(e.to_string()))?;
        return v
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())
            .ok_or(LlmCallError::MissingContent);
    }

    Err(LlmCallError::Transport("LLM 重试失败".to_string()))
}

/// 使用 Tavily 工具的循环调用
/// 处理 LLM 的 tool_calls，调用 Tavily，然后继续对话直到获得最终回复
async fn call_with_tavily_tool_loop(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    request_body: &serde_json::Value,
    max_request_bytes: u64,
    tavily_api_key: &str,
) -> Result<String, LlmCallError> {
    let mut messages = request_body
        .get("messages")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();

    let model = request_body
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("gpt-4o-mini");

    let max_tokens = request_body.get("max_tokens").cloned();

    // 最多循环 5 次（防止无限循环）
    for _ in 0..5 {
        let mut body = json!({
            "model": model,
            "messages": messages,
            "tools": [tavily_tool_definition()],
            "tool_choice": "auto"
        });
        if let Some(max_tok) = &max_tokens {
            body["max_tokens"] = max_tok.clone();
        }

        let request_bytes = serde_json::to_vec(&body)
            .map_err(|e| LlmCallError::Parse(format!("序列化请求失败: {e}")))?;
        if (request_bytes.len() as u64) > max_request_bytes {
            return Err(LlmCallError::RequestTooLarge {
                request_bytes: request_bytes.len() as u64,
                limit_bytes: max_request_bytes,
            });
        }

        let timeout = std::time::Duration::from_secs(180);
        let max_attempts: usize = 3;
        let mut ok_text: Option<String> = None;

        for attempt in 0..max_attempts {
            let resp = match client
                .post(url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("Content-Type", "application/json")
                .body(request_bytes.clone())
                .timeout(timeout)
                .send()
                .await
            {
                Ok(resp) => resp,
                Err(e) => {
                    let err = LlmCallError::Transport(e.to_string());
                    if attempt + 1 >= max_attempts {
                        return Err(err);
                    }
                    let delay_ms = 300_u64
                        .saturating_mul(2_u64.saturating_pow(attempt as u32))
                        .min(3000);
                    warn!("LLM request failed, retrying in {}ms: {}", delay_ms, err);
                    sleep(std::time::Duration::from_millis(delay_ms)).await;
                    continue;
                }
            };

            let status = resp.status();
            let headers = resp.headers().clone();
            let text = resp
                .text()
                .await
                .map_err(|e| LlmCallError::Decode(e.to_string()))?;

            if !status.is_success() {
                let msg = serde_json::from_str::<serde_json::Value>(&text)
                    .ok()
                    .and_then(|v| {
                        v.get("error")
                            .cloned()
                            .or_else(|| v.get("message").cloned())
                    })
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| text.chars().take(400).collect());

                let http_status = status.as_u16();
                if attempt + 1 < max_attempts && should_retry_llm_http_status(http_status) {
                    let mut delay_ms = 500_u64
                        .saturating_mul(2_u64.saturating_pow(attempt as u32))
                        .min(5000);
                    if http_status == 429 {
                        let mut delay_secs = parse_retry_after_seconds_from_headers(&headers)
                            .or_else(|| parse_retry_after_seconds_from_message(&msg))
                            .unwrap_or(1);
                        delay_secs = delay_secs.clamp(1, 60);
                        delay_ms = delay_secs.saturating_mul(1000);
                    }
                    warn!(
                        "LLM HTTP {}, retrying in {}ms: {}",
                        http_status,
                        delay_ms,
                        msg.chars().take(140).collect::<String>()
                    );
                    sleep(std::time::Duration::from_millis(delay_ms)).await;
                    continue;
                }

                return Err(LlmCallError::Http {
                    status: http_status,
                    message: msg,
                });
            }

            ok_text = Some(text);
            break;
        }

        let Some(text) = ok_text else {
            return Err(LlmCallError::Transport("LLM 重试失败".to_string()));
        };

        let v: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| LlmCallError::Parse(e.to_string()))?;

        let choice = v
            .get("choices")
            .and_then(|c| c.get(0))
            .ok_or(LlmCallError::MissingContent)?;

        let message = choice.get("message").ok_or(LlmCallError::MissingContent)?;
        let finish_reason = choice
            .get("finish_reason")
            .and_then(|f| f.as_str())
            .unwrap_or("");

        // 检查是否有 tool_calls
        if let Some(tool_calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
            if !tool_calls.is_empty() {
                // 添加助手消息（包含 tool_calls）
                messages.push(message.clone());

                // 处理每个 tool call
                for tool_call in tool_calls {
                    let tool_call_id = tool_call
                        .get("id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("unknown");
                    let function = tool_call.get("function");
                    let function_name = function
                        .and_then(|f| f.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("");

                    if function_name == "tavily_search" {
                        let arguments = function
                            .and_then(|f| f.get("arguments"))
                            .and_then(|a| a.as_str())
                            .unwrap_or("{}");

                        let args: serde_json::Value =
                            serde_json::from_str(arguments).unwrap_or(json!({}));
                        let query = args.get("query").and_then(|q| q.as_str()).unwrap_or("");

                        info!("Tavily 搜索: {}", query);

                        // 调用 Tavily
                        let search_result = match call_tavily_search(tavily_api_key, query).await {
                            Ok(result) => result,
                            Err(e) => {
                                error!("Tavily 搜索失败: {}", e);
                                format!("搜索失败: {}", e)
                            }
                        };

                        // 添加工具响应消息
                        messages.push(json!({
                            "role": "tool",
                            "tool_call_id": tool_call_id,
                            "content": search_result
                        }));
                    }
                }

                // 继续循环，让 LLM 处理搜索结果
                continue;
            }
        }

        // 没有 tool_calls 或 finish_reason 是 stop，返回内容
        if finish_reason == "stop" || finish_reason == "end_turn" || finish_reason.is_empty() {
            if let Some(content) = message.get("content") {
                if let Some(s) = content.as_str() {
                    return Ok(s.to_string());
                }
                if let Some(parts) = content.as_array() {
                    let mut out = String::new();
                    for part in parts {
                        if let Some(s) = part.as_str() {
                            out.push_str(s);
                            continue;
                        }
                        if let Some(obj) = part.as_object() {
                            if let Some(t) = obj.get("text").and_then(|v| v.as_str()) {
                                out.push_str(t);
                            } else if let Some(t) = obj.get("content").and_then(|v| v.as_str()) {
                                out.push_str(t);
                            }
                        }
                    }
                    if !out.trim().is_empty() {
                        return Ok(out);
                    }
                }
            }
            return Err(LlmCallError::MissingContent);
        }

        // 其他情况也尝试返回内容
        if let Some(content) = message.get("content") {
            if let Some(s) = content.as_str() {
                if !s.trim().is_empty() {
                    return Ok(s.to_string());
                }
            }
        }
    }

    Err(LlmCallError::MissingContent)
}

fn guess_transcription_mime(file_name: &str) -> &'static str {
    let ext = Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "mpeg" | "mpg" | "mpga" => "audio/mpeg",
        _ => "application/octet-stream",
    }
}

pub(super) async fn call_audio_transcription(
    base_url: &str,
    api_key: &str,
    model: &str,
    file_path: &Path,
    file_name: &str,
) -> Result<String, String> {
    let bytes = tokio::fs::read(file_path)
        .await
        .map_err(|e| format!("读取音频失败: {e}"))?;

    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(file_name.to_string())
        .mime_str(guess_transcription_mime(file_name))
        .map_err(|e| format!("构造音频 multipart 失败: {e}"))?;

    let form = reqwest::multipart::Form::new()
        .text("model", model.to_string())
        .text("language", "zh".to_string())
        .part("file", part);

    let client = reqwest::Client::new();
    let url = format!("{}/audio/transcriptions", base_url.trim_end_matches('/'));
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(form)
        .timeout(std::time::Duration::from_secs(300))
        .send()
        .await
        .map_err(|e| format!("音频转写请求失败: {}", e))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取转写响应失败: {}", e))?;

    if !status.is_success() {
        let msg = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .cloned()
                    .or_else(|| v.get("message").cloned())
            })
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| text.chars().take(400).collect());
        return Err(format!("音频转写失败 (HTTP {}): {}", status, msg));
    }

    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("解析转写响应失败: {e}"))?;
    v.get("text")
        .or_else(|| v.get("transcript"))
        .or_else(|| v.get("data").and_then(|d| d.get("text")))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "音频转写失败：无法获取文本".to_string())
}

pub(super) fn nonce12() -> String {
    use rand::distr::Alphanumeric;
    use rand::Rng;
    rand::rng()
        .sample_iter(Alphanumeric)
        .take(12)
        .map(char::from)
        .collect()
}

pub(in super::super) async fn reply_err(
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    user_id: u64,
    group_id: u64,
    msg: &str,
) {
    send_reply(
        runtime,
        bot_id,
        user_id,
        (group_id != 0).then_some(group_id),
        msg,
    )
    .await;
}

pub(in super::super) fn log_llm_len(kind: &str, len: usize) {
    info!("LLM {}完成，回复长度: {}", kind, len);
}

pub(in super::super) fn log_llm_error(kind: &str, err: &str) {
    error!("LLM {}失败: {}", kind, err);
}
