use serde_json::json;
use std::sync::Arc;

use crate::bot::runtime::BotRuntime;
use crate::models::SharedState;

use super::super::LlmForwardAudioFromUrlInput;
use super::common::{
    call_chat_completions, download_binary_to_temp, get_record_base64_as_temp, log_llm_error,
    log_llm_len, read_file_as_data_url, reply_err, resolve_llm_config_by_name,
    send_llm_markdown_as_forward_image, SendForwardImageInput,
};

fn is_http_url(url: &str) -> bool {
    let u = url.trim();
    u.starts_with("http://") || u.starts_with("https://")
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

pub(in super::super::super) async fn process_llm_forward_audio_from_url(
    state: &SharedState,
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    input: LlmForwardAudioFromUrlInput<'_>,
) {
    let user_id = input.user_id;
    let group_id = input.group_id;
    let system_prompt = input.system_prompt;
    let prompt = input.prompt;
    let title = input.title;

    // Prefer OneBot `get_record` when available (handles silk/amr and returns a standard format).
    let (guard, bin_meta) = if let Some(record_file) = input.record_file {
        match get_record_base64_as_temp(
            runtime,
            bot_id,
            record_file,
            Some("record.wav"),
            input.max_bytes,
        )
        .await
        {
            Ok(v) => v,
            Err(e) => {
                reply_err(
                    runtime,
                    bot_id,
                    user_id,
                    group_id,
                    &format!("获取语音失败：{e}"),
                )
                .await;
                return;
            }
        }
    } else if is_http_url(input.url) {
        match download_binary_to_temp(
            input.url,
            input.file_name,
            input.timeout_ms,
            input.max_bytes,
        )
        .await
        {
            Ok(v) => v,
            Err(e) => {
                reply_err(
                    runtime,
                    bot_id,
                    user_id,
                    group_id,
                    &format!("下载失败：{e}"),
                )
                .await;
                return;
            }
        }
    } else {
        reply_err(
            runtime,
            bot_id,
            user_id,
            group_id,
            "无法获取语音：缺少可下载 URL，且未提供 record_file（无法调用 get_record）",
        )
        .await;
        return;
    };

    let llm = match resolve_llm_config_by_name(state, bot_id, input.model_name) {
        Ok(v) => v,
        Err(e) => {
            reply_err(runtime, bot_id, user_id, group_id, &e).await;
            return;
        }
    };

    let effective_prompt = if input.require_transcript {
        format!(
            "请先输出该音频的逐字转写内容（逐字、不翻译）。然后完成任务：{}",
            prompt
        )
    } else {
        prompt.to_string()
    };

    let mime_name = if input.record_file.is_some() {
        "record.wav".to_string()
    } else {
        bin_meta
            .file_name
            .as_deref()
            .map(|s| s.to_string())
            .or_else(|| input.file_name.map(|s| s.to_string()))
            .or_else(|| guess_file_name_from_url(input.url))
            .unwrap_or_else(|| "audio.wav".to_string())
    };

    let data_url = match read_file_as_data_url(&guard.path, &mime_name).await {
        Ok(v) => v,
        Err(e) => {
            reply_err(
                runtime,
                bot_id,
                user_id,
                group_id,
                &format!("读取音频失败：{e}"),
            )
            .await;
            return;
        }
    };

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

    let ctx = json!({
        "task": effective_prompt,
        "title": title,
        "document": {
            "type": "audio",
            "mode": "direct",
            "file_ext": bin_meta.file_ext,
            "size_bytes": bin_meta.size_bytes,
            "truncated": bin_meta.truncated,
            "max_audio_seconds": input.max_audio_seconds,
            "require_transcript": input.require_transcript,
            "input_mode": "image_url_data"
        },
        "environment": {
            "bot_id": bot_id,
            "bot_name": bot_name,
            "platform": bot_platform,
            "chat_type": if group_id_opt.is_some() { "group" } else { "private" },
            "time": now.to_rfc3339(),
        }
    });
    let ctx_pretty = serde_json::to_string_pretty(&ctx).unwrap_or_else(|_| ctx.to_string());

    let request_body = json!({
        "model": llm.model_name,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "system", "content": super::super::build_prompt_injection_guard()},
            {"role": "user", "content": [
                {"type": "text", "text": format!("上下文信息（JSON）：\n{}", ctx_pretty)},
                {"type": "image_url", "image_url": {"url": data_url}}
            ]}
        ],
        "max_tokens": 4096
    });

    let reply_content = match call_chat_completions(
        &llm.base_url,
        &llm.api_key,
        &request_body,
        llm.max_request_bytes,
    )
    .await
    {
        Ok(t) => t,
        Err(e) => {
            log_llm_error("语音分析", &e.to_string());
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
    };

    log_llm_len("语音分析", reply_content.len());
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

    drop(guard);
}
