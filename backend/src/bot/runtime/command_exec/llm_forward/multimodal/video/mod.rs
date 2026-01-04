mod ctx;
mod direct;
mod ffmpeg;
mod frames;

use serde_json::json;
use std::sync::Arc;
use tracing::warn;

use crate::bot::runtime::BotRuntime;
use crate::models::SharedState;

use super::super::LlmForwardVideoFromUrlInput;
use super::common::{
    call_audio_transcription, call_chat_completions, download_binary_to_temp, log_llm_error,
    log_llm_len, reply_err, resolve_llm_config_by_name, send_llm_markdown_as_forward_image,
    SendForwardImageInput,
};

use ctx::{build_video_ctx, VideoCtxInput};
use direct::prepare_video_data_url_with_budget;
use frames::{
    evenly_spaced_indices, extract_audio_wav, extract_video_frames_as_data_urls,
    probe_video_duration_seconds,
};

pub(in super::super::super) async fn process_llm_forward_video_from_url(
    state: &SharedState,
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    input: LlmForwardVideoFromUrlInput<'_>,
) {
    let user_id = input.user_id;
    let group_id = input.group_id;
    let system_prompt = input.system_prompt;
    let prompt = input.prompt;
    let title = input.title;
    let mode = input.mode.trim().to_ascii_lowercase();

    let (guard, bin_meta) = match download_binary_to_temp(
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
    };

    let llm = match resolve_llm_config_by_name(state, bot_id, input.model_name) {
        Ok(v) => v,
        Err(e) => {
            reply_err(runtime, bot_id, user_id, group_id, &e).await;
            return;
        }
    };

    if mode == "direct" {
        let mime_name = bin_meta
            .file_name
            .as_deref()
            .or_else(|| guard.path.file_name().and_then(|n| n.to_str()))
            .unwrap_or("video.mp4");

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

        let ctx_base = json!({
            "task": prompt,
            "title": title,
            "document": {
                "type": "video",
                "mode": "direct",
                "file_ext": bin_meta.file_ext,
                "size_bytes": bin_meta.size_bytes,
                "truncated": bin_meta.truncated,
                "transcribe_audio_requested": input.transcribe_audio,
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
        let ctx_base_pretty =
            serde_json::to_string_pretty(&ctx_base).unwrap_or_else(|_| ctx_base.to_string());

        let overhead_request = json!({
            "model": llm.model_name,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "system", "content": super::super::build_prompt_injection_guard()},
                {"role": "user", "content": [
                    {"type": "text", "text": format!("上下文信息（JSON）：\n{}", ctx_base_pretty)},
                    {"type": "image_url", "image_url": {"url": "data:video/mp4;base64,"}}
                ]}
            ],
            "max_tokens": 4096
        });
        let overhead_bytes = serde_json::to_vec(&overhead_request)
            .map(|v| v.len() as u64)
            .unwrap_or(llm.max_request_bytes);
        let base64_budget = llm
            .max_request_bytes
            .saturating_sub(overhead_bytes.saturating_add(32 * 1024));
        let mut max_raw_bytes = (base64_budget / 4) * 3;
        if max_raw_bytes < 80_000 {
            reply_err(
                runtime,
                bot_id,
                user_id,
                group_id,
                "提供商的 max_request_bytes 过小，无法进行视频分析（请求体预算不足）",
            )
            .await;
            return;
        }

        let mut last_err: Option<String> = None;
        for _attempt in 0..3 {
            let (data_url, prepared) =
                match prepare_video_data_url_with_budget(&guard.path, mime_name, max_raw_bytes)
                    .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        last_err = Some(e);
                        break;
                    }
                };

            let ctx = json!({
                "task": prompt,
                "title": title,
                "document": {
                    "type": "video",
                    "mode": "direct",
                    "file_ext": bin_meta.file_ext,
                    "size_bytes": bin_meta.size_bytes,
                    "truncated": bin_meta.truncated,
                    "prepared": {
                        "original_bytes": prepared.original_bytes,
                        "transcoded": prepared.transcoded,
                        "prepared_bytes": prepared.prepared_bytes,
                    },
                    "transcribe_audio_requested": input.transcribe_audio,
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
                    let retryable =
                        matches!(&e, super::common::LlmCallError::RequestTooLarge { .. })
                            || e.http_status() == Some(413);
                    if retryable {
                        last_err = Some(e.to_string());
                        max_raw_bytes = (max_raw_bytes as f64 * 0.7) as u64;
                        continue;
                    }

                    log_llm_error("视频分析", &e.to_string());
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

            log_llm_len("视频分析", reply_content.len());
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
            return;
        }

        let msg = last_err.unwrap_or_else(|| "分析失败：视频处理失败".to_string());
        reply_err(runtime, bot_id, user_id, group_id, &msg).await;
        return;
    }

    let frames = match extract_video_frames_as_data_urls(
        &guard.path,
        input.max_frames,
        input.frame_max_width,
        input.frame_max_height,
        input.frame_jpeg_quality,
        input.frame_max_output_bytes,
    )
    .await
    {
        Ok(v) if !v.is_empty() => v,
        Ok(_) => {
            reply_err(
                runtime,
                bot_id,
                user_id,
                group_id,
                "视频抽帧失败：未提取到任何帧（可能是不支持的格式或 ffmpeg 不可用）",
            )
            .await;
            return;
        }
        Err(e) => {
            reply_err(
                runtime,
                bot_id,
                user_id,
                group_id,
                &format!("视频抽帧失败：{e}"),
            )
            .await;
            return;
        }
    };

    let transcript = if input.transcribe_audio {
        let transcription_model = input.transcription_model.unwrap_or("whisper-1");
        match extract_audio_wav(&guard.path, input.max_audio_seconds).await {
            Ok(audio) => match call_audio_transcription(
                &llm.base_url,
                &llm.api_key,
                transcription_model,
                &audio.path,
                "audio.wav",
            )
            .await
            {
                Ok(t) => Some(t),
                Err(e) => {
                    if input.require_transcript {
                        reply_err(
                            runtime,
                            bot_id,
                            user_id,
                            group_id,
                            &format!("音频转写失败（已设置为必须成功）：{e}"),
                        )
                        .await;
                        return;
                    }
                    warn!("音频转写失败，将继续仅基于画面分析: {}", e);
                    None
                }
            },
            Err(e) => {
                if input.require_transcript {
                    reply_err(
                        runtime,
                        bot_id,
                        user_id,
                        group_id,
                        &format!("提取音频失败（已设置为必须成功）：{e}"),
                    )
                    .await;
                    return;
                }
                warn!("提取音频失败，将继续仅基于画面分析: {}", e);
                None
            }
        }
    } else {
        None
    };

    let duration = probe_video_duration_seconds(&guard.path).await;
    let frames_total = frames.len();
    let mut keep = frames_total;
    let mut last_err: Option<String> = None;

    for _attempt in 0..4 {
        if keep == 0 {
            break;
        }
        let indices = evenly_spaced_indices(frames_total, keep);
        let ctx_frames = indices
            .iter()
            .filter_map(|&i| {
                frames
                    .get(i)
                    .map(|(ts_ms, _url, meta)| (*ts_ms, meta.clone()))
            })
            .collect::<Vec<_>>();
        if ctx_frames.is_empty() {
            break;
        }

        let ctx = build_video_ctx(VideoCtxInput {
            state,
            bot_id,
            group_id,
            prompt,
            title,
            bin_meta: &bin_meta,
            duration_seconds: duration,
            frames: &ctx_frames,
            frames_total,
            frames_selected: ctx_frames.len(),
            transcript_included: transcript.is_some(),
        });
        let ctx_pretty = serde_json::to_string_pretty(&ctx).unwrap_or_else(|_| ctx.to_string());

        let mut content_parts: Vec<serde_json::Value> = Vec::new();
        content_parts.push(json!({
            "type": "text",
            "text": format!("上下文信息（JSON）：\n{}", ctx_pretty)
        }));
        if let Some(t) = &transcript {
            let nonce = super::common::nonce12();
            let begin = format!("<<BEGIN_UNTRUSTED_AUDIO_TRANSCRIPT:{}>>", nonce);
            let end = format!("<<END_UNTRUSTED_AUDIO_TRANSCRIPT:{}>>", nonce);
            content_parts.push(json!({ "type": "text", "text": format!("{begin}\n{t}\n{end}") }));
        }
        for (pos, idx) in indices.iter().enumerate() {
            let Some((ts_ms, data_url, _meta)) = frames.get(*idx) else {
                continue;
            };
            content_parts.push(json!({
                "type": "text",
                "text": format!("Frame {} @ {}ms", pos + 1, ts_ms)
            }));
            content_parts.push(json!({ "type": "image_url", "image_url": { "url": data_url } }));
        }

        let request_body = json!({
            "model": llm.model_name,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "system", "content": super::super::build_prompt_injection_guard()},
                {"role": "user", "content": content_parts}
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
                let retryable = matches!(&e, super::common::LlmCallError::RequestTooLarge { .. })
                    || e.http_status() == Some(413);
                if retryable && keep > 1 {
                    last_err = Some(e.to_string());
                    keep = keep.div_ceil(2).max(1);
                    continue;
                }

                log_llm_error("视频分析", &e.to_string());
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

        log_llm_len("视频分析", reply_content.len());
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
        return;
    }

    let msg = last_err.unwrap_or_else(|| {
        "分析失败：请求体过大，无法在当前限制内发送视频帧（请减少帧数/降低画质或提高提供商的 max_request_bytes）".to_string()
    });
    reply_err(runtime, bot_id, user_id, group_id, &msg).await;
    drop(guard);
}
