use deno_core::{op2, OpState};

use super::state::ForwardNode;
use super::{MediaBundleItem, PluginOpState, PluginOutput};

#[derive(serde::Deserialize, Default)]
struct CallLlmChatPayload {
    request_id: String,
    #[serde(default)]
    model_name: Option<String>,
    messages: Vec<serde_json::Value>,
    #[serde(default)]
    max_tokens: Option<u32>,
}

// Op: 调用 LLM 进行多轮对话（异步返回结果）
#[op2(fast)]
pub(in super::super) fn op_call_llm_chat(
    state: &mut OpState,
    #[string] payload_json: &str,
) {
    let Some(payload) = super::parse_payload_or_reply::<CallLlmChatPayload>(
        state,
        0,
        0,
        "callLlmChat",
        payload_json,
    ) else {
        return;
    };

    if payload.request_id.trim().is_empty() {
        return;
    }

    if payload.messages.is_empty() {
        return;
    }

    state
        .borrow_mut::<PluginOpState>()
        .outputs
        .push(PluginOutput::CallLlmChat {
            request_id: payload.request_id,
            model_name: payload.model_name,
            messages: payload.messages,
            max_tokens: payload.max_tokens,
        });
}

#[derive(serde::Deserialize, Default)]
struct CallLlmChatWithSearchPayload {
    request_id: String,
    #[serde(default)]
    model_name: Option<String>,
    messages: Vec<serde_json::Value>,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    enable_search: Option<bool>,
}

// Op: 调用支持联网搜索的 LLM（异步返回结果）
#[op2(fast)]
pub(in super::super) fn op_call_llm_chat_with_search(
    state: &mut OpState,
    #[string] payload_json: &str,
) {
    let Some(payload) = super::parse_payload_or_reply::<CallLlmChatWithSearchPayload>(
        state,
        0,
        0,
        "callLlmChatWithSearch",
        payload_json,
    ) else {
        return;
    };

    if payload.request_id.trim().is_empty() {
        return;
    }

    if payload.messages.is_empty() {
        return;
    }

    state
        .borrow_mut::<PluginOpState>()
        .outputs
        .push(PluginOutput::CallLlmChatWithSearch {
            request_id: payload.request_id,
            model_name: payload.model_name,
            messages: payload.messages,
            max_tokens: payload.max_tokens,
            enable_search: payload.enable_search,
        });
}

#[derive(serde::Deserialize, Default)]
struct SendForwardMessagePayload {
    #[serde(default)]
    nodes: Vec<ForwardNode>,
}

// Op: 发送合并转发消息
#[op2(fast)]
pub(in super::super) fn op_send_forward_message(
    state: &mut OpState,
    #[bigint] user_id: i64,
    #[bigint] group_id: i64,
    #[string] payload_json: &str,
) {
    let Some(payload) = super::parse_payload_or_reply::<SendForwardMessagePayload>(
        state,
        user_id,
        group_id,
        "sendForwardMessage",
        payload_json,
    ) else {
        return;
    };

    if payload.nodes.is_empty() {
        return;
    }

    state
        .borrow_mut::<PluginOpState>()
        .outputs
        .push(PluginOutput::SendForwardMessage {
            user_id: user_id as u64,
            group_id: group_id as u64,
            nodes: payload.nodes,
        });
}

// Op: 调用 LLM 并发送合并转发消息
#[op2(fast)]
pub(in super::super) fn op_call_llm_forward(
    state: &mut OpState,
    #[bigint] user_id: i64,
    #[bigint] group_id: i64,
    #[string] system_prompt: &str,
    #[string] prompt: &str,
    #[string] content: &str,
    #[string] title: &str,
) {
    state
        .borrow_mut::<PluginOpState>()
        .outputs
        .push(PluginOutput::CallLlmAndForward {
            user_id: user_id as u64,
            group_id: group_id as u64,
            model_name: None,
            system_prompt: system_prompt.to_string(),
            prompt: prompt.to_string(),
            content: content.to_string(),
            title: title.to_string(),
        });
}

#[derive(serde::Deserialize, Default)]
struct CallLlmForwardFromUrlPayload {
    #[serde(default)]
    model_name: Option<String>,
    url: String,
    title: String,
    #[serde(default)]
    file_name: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    max_bytes: Option<u64>,
    #[serde(default)]
    max_chars: Option<u64>,
}

// Op: 从 URL 下载内容后调用 LLM 并发送合并转发消息（临时文件，处理完即删除）
#[op2(fast)]
pub(in super::super) fn op_call_llm_forward_from_url(
    state: &mut OpState,
    #[bigint] user_id: i64,
    #[bigint] group_id: i64,
    #[string] system_prompt: &str,
    #[string] prompt: &str,
    #[string] payload_json: &str,
) {
    let Some(payload) = super::parse_payload_or_reply::<CallLlmForwardFromUrlPayload>(
        state,
        user_id,
        group_id,
        "callLlmForwardFromUrl",
        payload_json,
    ) else {
        return;
    };

    if payload.url.trim().is_empty() || payload.title.trim().is_empty() {
        super::push_reply(state, user_id, group_id, "插件内部错误：参数缺失");
        return;
    }

    state
        .borrow_mut::<PluginOpState>()
        .outputs
        .push(PluginOutput::CallLlmAndForwardFromUrl {
            user_id: user_id as u64,
            group_id: group_id as u64,
            model_name: payload.model_name.filter(|s| !s.trim().is_empty()),
            system_prompt: system_prompt.to_string(),
            prompt: prompt.to_string(),
            url: payload.url,
            title: payload.title,
            file_name: payload.file_name,
            timeout_ms: payload.timeout_ms.unwrap_or(30000).clamp(1000, 120000),
            max_bytes: payload
                .max_bytes
                .unwrap_or(2_000_000)
                .clamp(1024, 50_000_000),
            max_chars: payload.max_chars.unwrap_or(50_000).clamp(1000, 200_000),
        });
}

#[derive(serde::Deserialize, Default)]
struct CallLlmForwardImageFromUrlPayload {
    #[serde(default)]
    model_name: Option<String>,
    url: String,
    title: String,
    #[serde(default)]
    file_name: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    max_bytes: Option<u64>,
    #[serde(default)]
    max_width: Option<u32>,
    #[serde(default)]
    max_height: Option<u32>,
    #[serde(default)]
    jpeg_quality: Option<u8>,
    #[serde(default)]
    max_output_bytes: Option<u64>,
}

// Op: 从 URL 下载图片后调用多模态 LLM（临时文件，处理完即删除），并发送结果（合并转发）
#[op2(fast)]
pub(in super::super) fn op_call_llm_forward_image_from_url(
    state: &mut OpState,
    #[bigint] user_id: i64,
    #[bigint] group_id: i64,
    #[string] system_prompt: &str,
    #[string] prompt: &str,
    #[string] payload_json: &str,
) {
    let Some(payload) = super::parse_payload_or_reply::<CallLlmForwardImageFromUrlPayload>(
        state,
        user_id,
        group_id,
        "callLlmForwardImageFromUrl",
        payload_json,
    ) else {
        return;
    };

    if payload.url.trim().is_empty() || payload.title.trim().is_empty() {
        super::push_reply(state, user_id, group_id, "插件内部错误：参数缺失");
        return;
    }

    state
        .borrow_mut::<PluginOpState>()
        .outputs
        .push(PluginOutput::CallLlmAndForwardImageFromUrl {
            user_id: user_id as u64,
            group_id: group_id as u64,
            model_name: payload.model_name.filter(|s| !s.trim().is_empty()),
            system_prompt: system_prompt.to_string(),
            prompt: prompt.to_string(),
            url: payload.url,
            title: payload.title,
            file_name: payload.file_name,
            timeout_ms: payload.timeout_ms.unwrap_or(30000).clamp(1000, 120000),
            max_bytes: payload
                .max_bytes
                .unwrap_or(10_000_000)
                .clamp(10_000, 50_000_000),
            max_width: payload.max_width.unwrap_or(1024).clamp(320, 4096),
            max_height: payload.max_height.unwrap_or(1024).clamp(320, 4096),
            jpeg_quality: payload.jpeg_quality.unwrap_or(85).clamp(30, 95),
            max_output_bytes: payload
                .max_output_bytes
                .unwrap_or(2_000_000)
                .clamp(50_000, 10_000_000),
        });
}

#[derive(serde::Deserialize, Default)]
struct CallLlmForwardVideoFromUrlPayload {
    #[serde(default)]
    model_name: Option<String>,
    url: String,
    title: String,
    #[serde(default)]
    file_name: Option<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    max_bytes: Option<u64>,
    #[serde(default)]
    max_frames: Option<u32>,
    #[serde(default)]
    frame_max_width: Option<u32>,
    #[serde(default)]
    frame_max_height: Option<u32>,
    #[serde(default)]
    frame_jpeg_quality: Option<u8>,
    #[serde(default)]
    frame_max_output_bytes: Option<u64>,
    #[serde(default)]
    transcribe_audio: Option<bool>,
    #[serde(default)]
    transcription_model: Option<String>,
    #[serde(default)]
    max_audio_seconds: Option<u32>,
    #[serde(default)]
    require_transcript: Option<bool>,
}

// Op: 从 URL 下载视频后抽帧（可选转写音频）并调用多模态 LLM（临时文件，处理完即删除），发送结果（合并转发）
#[op2(fast)]
pub(in super::super) fn op_call_llm_forward_video_from_url(
    state: &mut OpState,
    #[bigint] user_id: i64,
    #[bigint] group_id: i64,
    #[string] system_prompt: &str,
    #[string] prompt: &str,
    #[string] payload_json: &str,
) {
    let Some(payload) = super::parse_payload_or_reply::<CallLlmForwardVideoFromUrlPayload>(
        state,
        user_id,
        group_id,
        "callLlmForwardVideoFromUrl",
        payload_json,
    ) else {
        return;
    };

    if payload.url.trim().is_empty() || payload.title.trim().is_empty() {
        super::push_reply(state, user_id, group_id, "插件内部错误：参数缺失");
        return;
    }

    state
        .borrow_mut::<PluginOpState>()
        .outputs
        .push(PluginOutput::CallLlmAndForwardVideoFromUrl {
            user_id: user_id as u64,
            group_id: group_id as u64,
            model_name: payload.model_name.filter(|s| !s.trim().is_empty()),
            system_prompt: system_prompt.to_string(),
            prompt: prompt.to_string(),
            url: payload.url,
            title: payload.title,
            file_name: payload.file_name,
            mode: payload.mode.unwrap_or_else(|| "direct".to_string()),
            timeout_ms: payload.timeout_ms.unwrap_or(30000).clamp(1000, 120000),
            max_bytes: payload
                .max_bytes
                .unwrap_or(50_000_000)
                .clamp(100_000, 200_000_000),
            max_frames: payload.max_frames.unwrap_or(12).clamp(1, 24),
            frame_max_width: payload.frame_max_width.unwrap_or(1024).clamp(320, 4096),
            frame_max_height: payload.frame_max_height.unwrap_or(1024).clamp(320, 4096),
            frame_jpeg_quality: payload.frame_jpeg_quality.unwrap_or(80).clamp(30, 95),
            frame_max_output_bytes: payload
                .frame_max_output_bytes
                .unwrap_or(600_000)
                .clamp(50_000, 10_000_000),
            transcribe_audio: payload.transcribe_audio.unwrap_or(true),
            transcription_model: payload.transcription_model,
            max_audio_seconds: payload.max_audio_seconds.unwrap_or(180).clamp(10, 1800),
            require_transcript: payload.require_transcript.unwrap_or(false),
        });
}

#[derive(serde::Deserialize, Default)]
struct CallLlmForwardAudioFromUrlPayload {
    #[serde(default)]
    model_name: Option<String>,
    url: String,
    title: String,
    #[serde(default)]
    file_name: Option<String>,
    #[serde(default)]
    record_file: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    max_bytes: Option<u64>,
    #[serde(default)]
    max_audio_seconds: Option<u32>,
    #[serde(default)]
    require_transcript: Option<bool>,
}

// Op: 从 URL 下载音频后调用多模态 LLM（临时文件，处理完即删除），发送结果（合并转发）
#[op2(fast)]
pub(in super::super) fn op_call_llm_forward_audio_from_url(
    state: &mut OpState,
    #[bigint] user_id: i64,
    #[bigint] group_id: i64,
    #[string] system_prompt: &str,
    #[string] prompt: &str,
    #[string] payload_json: &str,
) {
    let Some(payload) = super::parse_payload_or_reply::<CallLlmForwardAudioFromUrlPayload>(
        state,
        user_id,
        group_id,
        "callLlmForwardAudioFromUrl",
        payload_json,
    ) else {
        return;
    };

    if payload.title.trim().is_empty() {
        super::push_reply(state, user_id, group_id, "插件内部错误：参数缺失");
        return;
    }
    let has_url = !payload.url.trim().is_empty();
    let has_record_file = payload
        .record_file
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    if !has_url && !has_record_file {
        super::push_reply(
            state,
            user_id,
            group_id,
            "插件内部错误：缺少 url 或 record_file",
        );
        return;
    }

    state
        .borrow_mut::<PluginOpState>()
        .outputs
        .push(PluginOutput::CallLlmAndForwardAudioFromUrl {
            user_id: user_id as u64,
            group_id: group_id as u64,
            model_name: payload.model_name.filter(|s| !s.trim().is_empty()),
            system_prompt: system_prompt.to_string(),
            prompt: prompt.to_string(),
            url: payload.url,
            title: payload.title,
            file_name: payload.file_name,
            record_file: payload.record_file,
            timeout_ms: payload.timeout_ms.unwrap_or(30000).clamp(1000, 120000),
            max_bytes: payload
                .max_bytes
                .unwrap_or(20_000_000)
                .clamp(10_000, 200_000_000),
            max_audio_seconds: payload.max_audio_seconds.unwrap_or(180).clamp(10, 1800),
            require_transcript: payload.require_transcript.unwrap_or(false),
        });
}

#[derive(serde::Deserialize, Default)]
struct CallLlmForwardMediaBundlePayload {
    #[serde(default)]
    model_name: Option<String>,
    title: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    items: Vec<MediaBundleItem>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    // Image options
    #[serde(default)]
    image_max_bytes: Option<u64>,
    #[serde(default)]
    image_max_width: Option<u32>,
    #[serde(default)]
    image_max_height: Option<u32>,
    #[serde(default)]
    image_jpeg_quality: Option<u8>,
    #[serde(default)]
    image_max_output_bytes: Option<u64>,
    // Video / Audio options
    #[serde(default)]
    video_max_bytes: Option<u64>,
    #[serde(default)]
    audio_max_bytes: Option<u64>,
}

// Op: 多媒体 bundle（文本 + 多个附件）调用多模态 LLM 并发送结果（合并转发）
#[op2(fast)]
pub(in super::super) fn op_call_llm_forward_media_bundle(
    state: &mut OpState,
    #[bigint] user_id: i64,
    #[bigint] group_id: i64,
    #[string] system_prompt: &str,
    #[string] prompt: &str,
    #[string] payload_json: &str,
) {
    let Some(payload) = super::parse_payload_or_reply::<CallLlmForwardMediaBundlePayload>(
        state,
        user_id,
        group_id,
        "callLlmForwardMediaBundle",
        payload_json,
    ) else {
        return;
    };

    if payload.title.trim().is_empty() {
        super::push_reply(state, user_id, group_id, "插件内部错误：参数缺失");
        return;
    }

    let mut items = payload.items;
    if items.len() > 20 {
        items.truncate(20);
    }

    state
        .borrow_mut::<PluginOpState>()
        .outputs
        .push(PluginOutput::CallLlmAndForwardMediaBundle {
            user_id: user_id as u64,
            group_id: group_id as u64,
            model_name: payload.model_name.filter(|s| !s.trim().is_empty()),
            system_prompt: system_prompt.to_string(),
            prompt: prompt.to_string(),
            title: payload.title,
            text: payload.text,
            items,
            timeout_ms: payload.timeout_ms.unwrap_or(30000).clamp(1000, 120000),
            image_max_bytes: payload
                .image_max_bytes
                .unwrap_or(10_000_000)
                .clamp(10_000, 50_000_000),
            image_max_width: payload.image_max_width.unwrap_or(1024).clamp(320, 4096),
            image_max_height: payload.image_max_height.unwrap_or(1024).clamp(320, 4096),
            image_jpeg_quality: payload.image_jpeg_quality.unwrap_or(85).clamp(30, 95),
            image_max_output_bytes: payload
                .image_max_output_bytes
                .unwrap_or(2_000_000)
                .clamp(50_000, 10_000_000),
            video_max_bytes: payload
                .video_max_bytes
                .unwrap_or(50_000_000)
                .clamp(100_000, 200_000_000),
            audio_max_bytes: payload
                .audio_max_bytes
                .unwrap_or(20_000_000)
                .clamp(10_000, 200_000_000),
        });
}
