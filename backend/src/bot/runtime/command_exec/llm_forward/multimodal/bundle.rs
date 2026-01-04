use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use tracing::warn;

use crate::bot::runtime::BotRuntime;
use crate::models::SharedState;

use super::super::LlmForwardMediaBundleInput;
use super::common::{
    call_chat_completions, download_binary_to_temp, get_record_base64_as_temp, log_llm_error,
    log_llm_len, nonce12, read_file_as_data_url, reply_err, resolve_llm_config_by_name,
    send_llm_markdown_as_forward_image, BinaryMeta, SendForwardImageInput,
};
use super::image::{prepare_image_data_url, PreparedImageMeta};

fn best_file_name(meta: &BinaryMeta, fallback: &str) -> String {
    meta.file_name
        .as_deref()
        .unwrap_or(fallback)
        .trim()
        .to_string()
}

fn prepared_image_meta_json(meta: &PreparedImageMeta) -> serde_json::Value {
    json!({
        "mime": meta.mime,
        "width": meta.width,
        "height": meta.height,
        "bytes": meta.output_bytes,
        "quality": meta.quality,
    })
}

pub(in super::super::super) async fn process_llm_forward_media_bundle(
    state: &SharedState,
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    input: LlmForwardMediaBundleInput<'_>,
) {
    let user_id = input.user_id;
    let group_id = input.group_id;
    let system_prompt = input.system_prompt;
    let prompt = input.prompt;
    let title = input.title;

    if input.items.is_empty() && input.text.map(|s| s.trim().is_empty()).unwrap_or(true) {
        reply_err(
            runtime,
            bot_id,
            user_id,
            group_id,
            "无法分析：未提供文本，且没有可用的媒体附件",
        )
        .await;
        return;
    }

    let llm = match resolve_llm_config_by_name(state, bot_id, input.model_name) {
        Ok(v) => v,
        Err(e) => {
            reply_err(runtime, bot_id, user_id, group_id, &e).await;
            return;
        }
    };

    let mut media_meta: Vec<serde_json::Value> = Vec::new();
    let mut file_meta: Vec<serde_json::Value> = Vec::new();
    let mut attachment_parts: Vec<serde_json::Value> = Vec::new();
    let mut failures: Vec<serde_json::Value> = Vec::new();

    for (idx, item) in input.items.iter().enumerate() {
        let kind = item.kind.trim().to_ascii_lowercase();
        let idx1 = idx + 1;

        match kind.as_str() {
            "image" => {
                let Some(url) = item.url.as_deref() else {
                    failures.push(json!({"index": idx1, "type": kind, "error": "missing url"}));
                    continue;
                };

                let (guard, meta) = match download_binary_to_temp(
                    url,
                    item.name.as_deref().or(item.file.as_deref()),
                    input.timeout_ms,
                    input.image_max_bytes,
                )
                .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        failures.push(json!({"index": idx1, "type": kind, "error": e}));
                        continue;
                    }
                };

                let (data_url, prepared_meta) = match prepare_image_data_url(
                    &guard.path,
                    input.image_max_width,
                    input.image_max_height,
                    input.image_jpeg_quality,
                    input.image_max_output_bytes,
                )
                .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        failures.push(json!({"index": idx1, "type": kind, "error": e}));
                        drop(guard);
                        continue;
                    }
                };

                media_meta.push(json!({
                    "index": idx1,
                    "type": "image",
                    "file_ext": meta.file_ext,
                    "size_bytes": meta.size_bytes,
                    "truncated": meta.truncated,
                    "prepared": prepared_image_meta_json(&prepared_meta),
                }));

                attachment_parts.push(json!({
                    "type": "text",
                    "text": format!("附件 #{idx1}: 图片")
                }));
                attachment_parts.push(json!({
                    "type": "image_url",
                    "image_url": { "url": data_url }
                }));
                drop(guard);
            }
            "video" => {
                let Some(url) = item.url.as_deref() else {
                    failures.push(json!({"index": idx1, "type": kind, "error": "missing url"}));
                    continue;
                };

                let (guard, meta) = match download_binary_to_temp(
                    url,
                    item.name.as_deref().or(item.file.as_deref()),
                    input.timeout_ms,
                    input.video_max_bytes,
                )
                .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        failures.push(json!({"index": idx1, "type": kind, "error": e}));
                        continue;
                    }
                };

                let mime_name = best_file_name(&meta, "video.mp4");
                let data_url = match read_file_as_data_url(&guard.path, &mime_name).await {
                    Ok(v) => v,
                    Err(e) => {
                        failures.push(json!({"index": idx1, "type": kind, "error": e}));
                        drop(guard);
                        continue;
                    }
                };

                media_meta.push(json!({
                    "index": idx1,
                    "type": "video",
                    "file_ext": meta.file_ext,
                    "size_bytes": meta.size_bytes,
                    "truncated": meta.truncated,
                    "mode": "direct",
                }));

                attachment_parts.push(json!({
                    "type": "text",
                    "text": format!("附件 #{idx1}: 视频")
                }));
                // NOTE: For current OpenAI-compatible gateway, video/audio are accepted via `image_url` with a data: URL.
                attachment_parts.push(json!({
                    "type": "image_url",
                    "image_url": { "url": data_url }
                }));

                drop(guard);
            }
            "record" => {
                let record_file = item.file.as_deref();
                let record_url = item.url.as_deref();
                let (guard, meta) = if let Some(rf) = record_file {
                    match get_record_base64_as_temp(
                        runtime,
                        bot_id,
                        rf,
                        Some("record.wav"),
                        input.audio_max_bytes,
                    )
                    .await
                    {
                        Ok((g, m)) => (g, m),
                        Err(e) => {
                            failures.push(json!({"index": idx1, "type": kind, "error": e}));
                            continue;
                        }
                    }
                } else if let Some(url) = record_url {
                    match download_binary_to_temp(
                        url,
                        item.name.as_deref().or(item.file.as_deref()),
                        input.timeout_ms,
                        input.audio_max_bytes,
                    )
                    .await
                    {
                        Ok((g, m)) => (g, m),
                        Err(e) => {
                            failures.push(json!({"index": idx1, "type": kind, "error": e}));
                            continue;
                        }
                    }
                } else {
                    failures.push(json!({"index": idx1, "type": kind, "error": "missing record_file and url"}));
                    continue;
                };

                let mime_name = if record_file.is_some() {
                    "record.wav".to_string()
                } else {
                    best_file_name(&meta, "record.wav")
                };
                let data_url = match read_file_as_data_url(&guard.path, &mime_name).await {
                    Ok(v) => v,
                    Err(e) => {
                        failures.push(json!({"index": idx1, "type": kind, "error": e}));
                        drop(guard);
                        continue;
                    }
                };

                media_meta.push(json!({
                    "index": idx1,
                    "type": "audio",
                    "file_ext": meta.file_ext,
                    "size_bytes": meta.size_bytes,
                    "truncated": meta.truncated,
                    "mode": "direct",
                }));

                attachment_parts.push(json!({
                    "type": "text",
                    "text": format!("附件 #{idx1}: 语音/音频")
                }));
                attachment_parts.push(json!({
                    "type": "image_url",
                    "image_url": { "url": data_url }
                }));

                drop(guard);
            }
            "file" => {
                // 文件附件（非图片/视频/语音）：目前仅提供元信息，不自动下载/注入内容，避免提示词注入与二进制误读。
                let ext = item
                    .name
                    .as_deref()
                    .or(item.file.as_deref())
                    .and_then(|s| Path::new(s).extension().and_then(|e| e.to_str()))
                    .map(|s| s.to_ascii_lowercase());
                file_meta.push(json!({
                    "index": idx1,
                    "type": "file",
                    "file_ext": ext,
                    "has_url": item.url.as_ref().map(|s| !s.trim().is_empty()).unwrap_or(false),
                }));
            }
            other => {
                warn!("Unsupported bundle item type: {}", other);
                failures.push(json!({"index": idx1, "type": other, "error": "unsupported type"}));
            }
        }
    }

    if media_meta.is_empty()
        && file_meta.is_empty()
        && input.text.map(|s| s.trim().is_empty()).unwrap_or(true)
    {
        reply_err(
            runtime,
            bot_id,
            user_id,
            group_id,
            "无法分析：未能获取到任何可用的媒体附件（可能下载失败或类型不支持）",
        )
        .await;
        return;
    }

    let group_id_opt = (group_id != 0).then_some(group_id);
    let bot_name = state
        .bots
        .get(bot_id)
        .map(|b| b.value().name.clone())
        .unwrap_or_else(|| "nBot".to_string());
    let bot_platform = state
        .bots
        .get(bot_id)
        .map(|b| b.value().platform.clone())
        .unwrap_or_else(|| "unknown".to_string());
    let now = chrono::Local::now();

    let mut dropped: Vec<serde_json::Value> = Vec::new();

    let build_ctx_pretty = |media_meta: &[serde_json::Value],
                            file_meta: &[serde_json::Value],
                            failures: &[serde_json::Value],
                            dropped: &[serde_json::Value]| {
        let mut items: Vec<serde_json::Value> =
            Vec::with_capacity(media_meta.len() + file_meta.len());
        items.extend(media_meta.iter().cloned());
        items.extend(file_meta.iter().cloned());

        let ctx = json!({
            "task": prompt,
            "title": title,
            "document": {
                "type": "media_bundle",
                "text_included": input.text.is_some(),
                "items": items,
                "items_failed": failures,
                "items_dropped_due_to_budget": dropped,
            },
            "environment": {
                "bot_id": bot_id,
                "bot_name": bot_name,
                "platform": bot_platform,
                "chat_type": if group_id_opt.is_some() { "group" } else { "private" },
                "time": now.to_rfc3339(),
            }
        });
        serde_json::to_string_pretty(&ctx).unwrap_or_else(|_| ctx.to_string())
    };

    let mut ctx_pretty = build_ctx_pretty(&media_meta, &file_meta, &failures, &dropped);

    let mut content_parts: Vec<serde_json::Value> = Vec::new();
    content_parts.push(json!({
        "type": "text",
        "text": format!("上下文信息（JSON）：\n{}", ctx_pretty)
    }));

    if let Some(text) = input.text {
        let nonce = nonce12();
        let begin = format!("<<BEGIN_UNTRUSTED_FORWARD_TEXT:{}>>", nonce);
        let end = format!("<<END_UNTRUSTED_FORWARD_TEXT:{}>>", nonce);
        let text = super::super::redact::redact_qq_ids(text);
        content_parts.push(json!({
            "type": "text",
            "text": format!("{begin}\n{text}\n{end}")
        }));
    }

    if !failures.is_empty() {
        content_parts.push(json!({
            "type": "text",
            "text": format!("注意：有 {} 个附件因失败/不支持未能加入本次分析（详见上下文 JSON 的 items_failed）。", failures.len())
        }));
    }

    content_parts.extend(attachment_parts);

    let mut request_body = json!({
        "model": llm.model_name,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "system", "content": super::super::build_prompt_injection_guard()},
            {"role": "user", "content": content_parts}
        ],
        "max_tokens": 4096
    });

    let reply_content = loop {
        match call_chat_completions(
            &llm.base_url,
            &llm.api_key,
            &request_body,
            llm.max_request_bytes,
        )
        .await
        {
            Ok(t) => break t,
            Err(e) => {
                let retryable = matches!(&e, super::common::LlmCallError::RequestTooLarge { .. })
                    || e.http_status() == Some(413);

                if retryable {
                    let Some(removed) = media_meta.pop() else {
                        // Nothing left to drop; fall through and report the error.
                        log_llm_error("多媒体分析", &e.to_string());
                        reply_err(
                            runtime,
                            bot_id,
                            user_id,
                            group_id,
                            &format!("分析失败：{e}"),
                        )
                        .await;
                        return;
                    };

                    dropped.push(removed);

                    if let Some(arr) = request_body["messages"][2]["content"].as_array_mut() {
                        let _ = arr.pop();
                        let _ = arr.pop();
                    }

                    ctx_pretty = build_ctx_pretty(&media_meta, &file_meta, &failures, &dropped);
                    request_body["messages"][2]["content"][0]["text"] =
                        serde_json::Value::String(format!("上下文信息（JSON）：\n{}", ctx_pretty));
                    continue;
                }

                log_llm_error("多媒体分析", &e.to_string());
                reply_err(
                    runtime,
                    bot_id,
                    user_id,
                    group_id,
                    &format!("分析失败：{e}"),
                )
                .await;
                return;
            }
        }
    };

    log_llm_len("多媒体分析", reply_content.len());
    send_llm_markdown_as_forward_image(
        state,
        runtime,
        bot_id,
        SendForwardImageInput {
            user_id,
            group_id,
            title,
            markdown: &reply_content,
        },
    )
    .await;
}
