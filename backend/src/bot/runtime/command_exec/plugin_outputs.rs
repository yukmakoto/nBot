use crate::models::SharedState;
use crate::plugin::runtime::{ForwardNode, PluginOutput};
use crate::plugin::PluginOutputWithSource;
use serde_json::json;
use std::sync::Arc;
use tracing::warn;

use super::super::api::{send_api, send_reply};
use super::super::connection::{BotRuntime, GroupSendStatus};
use super::llm_abuse::{try_begin_llm_task, LlmAbuseConfig, LlmTaskGuard};
use super::llm_forward::{
    process_llm_forward, process_llm_forward_audio_from_url, process_llm_forward_image_from_url,
    process_llm_forward_media_bundle, process_llm_forward_video_from_url,
    LlmForwardAudioFromUrlInput, LlmForwardImageFromUrlInput, LlmForwardInput,
    LlmForwardMediaBundleInput, LlmForwardSource, LlmForwardVideoFromUrlInput,
};

/// 从 LLM 模块配置中获取 Tavily API key
fn get_tavily_api_key(state: &SharedState, bot_id: &str) -> Option<String> {
    crate::module::get_effective_module(state, bot_id, "llm").and_then(|m| {
        m.config
            .get("tavily_api_key")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    })
}

async fn begin_llm_task_guard(
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    abuse_cfg: LlmAbuseConfig,
    user_id: u64,
    group_id: u64,
) -> Option<LlmTaskGuard> {
    if group_id != 0
        && matches!(
            runtime.get_group_send_status(bot_id, group_id).await,
            GroupSendStatus::Muted
        )
    {
        warn!(
            "[{}] 群 {} 内机器人被禁言，跳过执行 LLM 任务",
            bot_id, group_id
        );
        return None;
    }

    match try_begin_llm_task(abuse_cfg, user_id, group_id) {
        Ok(g) => Some(g),
        Err(block) => {
            send_reply(
                runtime,
                bot_id,
                user_id,
                (group_id != 0).then_some(group_id),
                &block.message,
            )
            .await;
            None
        }
    }
}

async fn inline_multimodal_media_in_messages(
    messages: &mut Vec<serde_json::Value>,
    timeout_ms: u64,
    max_bytes: u64,
    max_width: u32,
    max_height: u32,
    jpeg_quality: u8,
    max_output_bytes: u64,
    max_images: usize,
) -> bool {
    use super::llm_forward::multimodal::common::download_and_prepare_media_data_url;

    let mut changed = false;
    let mut converted = 0usize;

    for msg in messages.iter_mut() {
        if converted >= max_images {
            break;
        }
        let Some(content) = msg.get_mut("content") else {
            continue;
        };
        let Some(parts) = content.as_array_mut() else {
            continue;
        };
        for part in parts.iter_mut() {
            if converted >= max_images {
                break;
            }
            let part_type = part
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if part_type != "image_url" {
                continue;
            }
            let Some(url) = part
                .get("image_url")
                .and_then(|v| v.get("url"))
                .and_then(|v| v.as_str())
            else {
                continue;
            };

            let url = url.trim();
            if url.starts_with("data:") {
                continue;
            }
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                continue;
            }

            match download_and_prepare_media_data_url(
                url,
                timeout_ms,
                max_bytes,
                max_width,
                max_height,
                jpeg_quality,
                max_output_bytes,
            )
            .await
            {
                Ok(data_url) => {
                    if let Some(obj) = part.as_object_mut() {
                        if let Some(img) = obj.get_mut("image_url").and_then(|v| v.as_object_mut())
                        {
                            img.insert("url".to_string(), serde_json::Value::String(data_url));
                            changed = true;
                            converted += 1;
                        }
                    }
                }
                Err(_e) => {
                    // Best-effort: keep original URL if conversion failed.
                }
            }
        }
    }

    changed
}

/// 处理插件输出
pub(super) async fn process_plugin_outputs(
    state: &SharedState,
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    outputs: &[PluginOutput],
) {
    let abuse_cfg = LlmAbuseConfig::from_state(state, bot_id);
    for output in outputs {
        match output {
            PluginOutput::UpdateConfig { plugin_id, config } => {
                if let Err(e) = state.plugins.update_config(plugin_id, config.clone()) {
                    warn!("[{}] 插件 {} 配置写入失败: {}", bot_id, plugin_id, e);
                }
                if let Err(e) = state
                    .plugin_manager
                    .update_config(plugin_id, config.clone())
                    .await
                {
                    warn!("[{}] 插件 {} 配置热更新失败: {}", bot_id, plugin_id, e);
                }
            }
            PluginOutput::SendReply {
                user_id,
                group_id,
                content,
            } => {
                send_reply(runtime, bot_id, *user_id, *group_id, content).await;
            }
            PluginOutput::CallApi { action, params } => {
                send_api(runtime, bot_id, action, params.clone()).await;
            }
            PluginOutput::CallLlmAndForward {
                user_id,
                group_id,
                model_name,
                system_prompt,
                prompt,
                content,
                title,
            } => {
                let Some(_guard) =
                    begin_llm_task_guard(runtime, bot_id, abuse_cfg, *user_id, *group_id).await
                else {
                    continue;
                };

                process_llm_forward(
                    state,
                    runtime,
                    bot_id,
                    LlmForwardInput {
                        user_id: *user_id,
                        group_id: *group_id,
                        model_name: model_name.as_deref(),
                        system_prompt,
                        prompt,
                        title,
                        source: LlmForwardSource::Content(content),
                    },
                )
                .await;
            }
            PluginOutput::CallLlmAndForwardFromUrl {
                user_id,
                group_id,
                model_name,
                system_prompt,
                prompt,
                url,
                title,
                file_name,
                timeout_ms,
                max_bytes,
                max_chars,
            } => {
                let Some(_guard) =
                    begin_llm_task_guard(runtime, bot_id, abuse_cfg, *user_id, *group_id).await
                else {
                    continue;
                };

                process_llm_forward(
                    state,
                    runtime,
                    bot_id,
                    LlmForwardInput {
                        user_id: *user_id,
                        group_id: *group_id,
                        model_name: model_name.as_deref(),
                        system_prompt,
                        prompt,
                        title,
                        source: LlmForwardSource::Url {
                            url,
                            file_name: file_name.as_deref(),
                            timeout_ms: *timeout_ms,
                            max_bytes: *max_bytes,
                            max_chars: *max_chars,
                        },
                    },
                )
                .await;
            }
            PluginOutput::CallLlmAndForwardImageFromUrl {
                user_id,
                group_id,
                model_name,
                system_prompt,
                prompt,
                url,
                title,
                file_name,
                timeout_ms,
                max_bytes,
                max_width,
                max_height,
                jpeg_quality,
                max_output_bytes,
            } => {
                let Some(_guard) =
                    begin_llm_task_guard(runtime, bot_id, abuse_cfg, *user_id, *group_id).await
                else {
                    continue;
                };

                process_llm_forward_image_from_url(
                    state,
                    runtime,
                    bot_id,
                    LlmForwardImageFromUrlInput {
                        user_id: *user_id,
                        group_id: *group_id,
                        model_name: model_name.as_deref(),
                        system_prompt,
                        prompt,
                        url,
                        title,
                        file_name: file_name.as_deref(),
                        timeout_ms: *timeout_ms,
                        max_bytes: *max_bytes,
                        max_width: *max_width,
                        max_height: *max_height,
                        jpeg_quality: *jpeg_quality,
                        max_output_bytes: *max_output_bytes,
                    },
                )
                .await;
            }
            PluginOutput::CallLlmAndForwardVideoFromUrl {
                user_id,
                group_id,
                model_name,
                system_prompt,
                prompt,
                url,
                title,
                file_name,
                mode,
                timeout_ms,
                max_bytes,
                max_frames,
                frame_max_width,
                frame_max_height,
                frame_jpeg_quality,
                frame_max_output_bytes,
                transcribe_audio,
                transcription_model,
                max_audio_seconds,
                require_transcript,
            } => {
                let Some(_guard) =
                    begin_llm_task_guard(runtime, bot_id, abuse_cfg, *user_id, *group_id).await
                else {
                    continue;
                };

                process_llm_forward_video_from_url(
                    state,
                    runtime,
                    bot_id,
                    LlmForwardVideoFromUrlInput {
                        user_id: *user_id,
                        group_id: *group_id,
                        model_name: model_name.as_deref(),
                        system_prompt,
                        prompt,
                        url,
                        title,
                        file_name: file_name.as_deref(),
                        mode,
                        timeout_ms: *timeout_ms,
                        max_bytes: *max_bytes,
                        max_frames: *max_frames,
                        frame_max_width: *frame_max_width,
                        frame_max_height: *frame_max_height,
                        frame_jpeg_quality: *frame_jpeg_quality,
                        frame_max_output_bytes: *frame_max_output_bytes,
                        transcribe_audio: *transcribe_audio,
                        transcription_model: transcription_model.as_deref(),
                        max_audio_seconds: *max_audio_seconds,
                        require_transcript: *require_transcript,
                    },
                )
                .await;
            }
            PluginOutput::CallLlmAndForwardAudioFromUrl {
                user_id,
                group_id,
                model_name,
                system_prompt,
                prompt,
                url,
                title,
                file_name,
                record_file,
                timeout_ms,
                max_bytes,
                max_audio_seconds,
                require_transcript,
            } => {
                let Some(_guard) =
                    begin_llm_task_guard(runtime, bot_id, abuse_cfg, *user_id, *group_id).await
                else {
                    continue;
                };

                process_llm_forward_audio_from_url(
                    state,
                    runtime,
                    bot_id,
                    LlmForwardAudioFromUrlInput {
                        user_id: *user_id,
                        group_id: *group_id,
                        model_name: model_name.as_deref(),
                        system_prompt,
                        prompt,
                        url,
                        title,
                        file_name: file_name.as_deref(),
                        record_file: record_file.as_deref(),
                        timeout_ms: *timeout_ms,
                        max_bytes: *max_bytes,
                        max_audio_seconds: *max_audio_seconds,
                        require_transcript: *require_transcript,
                    },
                )
                .await;
            }
            PluginOutput::CallLlmAndForwardMediaBundle {
                user_id,
                group_id,
                model_name,
                system_prompt,
                prompt,
                title,
                text,
                items,
                timeout_ms,
                image_max_bytes,
                image_max_width,
                image_max_height,
                image_jpeg_quality,
                image_max_output_bytes,
                video_max_bytes,
                audio_max_bytes,
            } => {
                let Some(_guard) =
                    begin_llm_task_guard(runtime, bot_id, abuse_cfg, *user_id, *group_id).await
                else {
                    continue;
                };

                process_llm_forward_media_bundle(
                    state,
                    runtime,
                    bot_id,
                    LlmForwardMediaBundleInput {
                        user_id: *user_id,
                        group_id: *group_id,
                        model_name: model_name.as_deref(),
                        system_prompt,
                        prompt,
                        title,
                        text: text.as_deref(),
                        items,
                        timeout_ms: *timeout_ms,
                        image_max_bytes: *image_max_bytes,
                        image_max_width: *image_max_width,
                        image_max_height: *image_max_height,
                        image_jpeg_quality: *image_jpeg_quality,
                        image_max_output_bytes: *image_max_output_bytes,
                        video_max_bytes: *video_max_bytes,
                        audio_max_bytes: *audio_max_bytes,
                    },
                )
                .await;
            }
            // CallLlmChat is handled in process_plugin_outputs_with_llm_response
            PluginOutput::CallLlmChat { .. } => {}
            // CallLlmChatWithSearch is handled in process_plugin_outputs_with_llm_response
            PluginOutput::CallLlmChatWithSearch { .. } => {}
            // Group info fetch outputs are handled in process_plugin_outputs_with_group_info_response
            PluginOutput::FetchGroupNotice { .. } => {}
            PluginOutput::FetchGroupMsgHistory { .. } => {}
            PluginOutput::FetchGroupFiles { .. } => {}
            PluginOutput::FetchGroupFileUrl { .. } => {}
            PluginOutput::FetchFriendList { .. } => {}
            PluginOutput::FetchGroupList { .. } => {}
            PluginOutput::FetchGroupMemberList { .. } => {}
            PluginOutput::DownloadFile { .. } => {}
            // SendForwardMessage sends merged forward message
            PluginOutput::SendForwardMessage {
                user_id,
                group_id,
                nodes,
            } => {
                send_forward_message(state, runtime, bot_id, *user_id, *group_id, nodes).await;
            }
        }
    }
}

/// 发送合并转发消息
async fn send_forward_message(
    state: &SharedState,
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    user_id: u64,
    group_id: u64,
    nodes: &[ForwardNode],
) {
    let bot_name = state
        .bots
        .get(bot_id)
        .map(|b| b.value().name.clone())
        .unwrap_or_else(|| "nBot".to_string());

    // Privacy: do not expose any real QQ number in forward nodes metadata.
    let bot_qq = 10000u64;

    let forward_nodes: Vec<serde_json::Value> = nodes
        .iter()
        .map(|node| {
            let name = if node.name.is_empty() {
                bot_name.clone()
            } else {
                node.name.clone()
            };
            let content = if node.content.is_null() {
                serde_json::Value::String(String::new())
            } else {
                node.content.clone()
            };
            json!({
                "type": "node",
                "data": {
                    "name": name,
                    "uin": bot_qq.to_string(),
                    "content": content
                }
            })
        })
        .collect();

    if group_id != 0 {
        send_api(
            runtime,
            bot_id,
            "send_group_forward_msg",
            json!({ "group_id": group_id, "messages": forward_nodes }),
        )
        .await;
    } else {
        send_api(
            runtime,
            bot_id,
            "send_private_forward_msg",
            json!({ "user_id": user_id, "messages": forward_nodes }),
        )
        .await;
    }
}

/// 处理插件输出，支持 LLM 回调
/// 当遇到 CallLlmChat 时，调用 LLM 并通过 onLlmResponse 钩子回调插件
/// plugin_id: 发起请求的插件 ID
pub(super) async fn process_plugin_outputs_with_llm_response(
    state: &SharedState,
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    plugin_id: &str,
    outputs: &[PluginOutput],
) {
    use super::llm_forward::multimodal::common::{
        call_chat_completions, call_chat_completions_with_tavily, resolve_llm_config_by_name,
    };

    for output in outputs {
        match output {
            PluginOutput::CallLlmChat {
                request_id,
                model_name,
                messages,
                max_tokens,
            } => {
                // 解析 LLM 配置
                let (success, content) =
                    match resolve_llm_config_by_name(state, bot_id, model_name.as_deref()) {
                        Ok(llm) => {
                            // If the plugin provided multimodal image_url parts, inline them as data URLs.
                            let mut prepared_messages = messages.clone();
                            let _ = inline_multimodal_media_in_messages(
                                &mut prepared_messages,
                                30_000,
                                15_000_000,
                                1024,
                                1024,
                                80,
                                600_000,
                                2,
                            )
                            .await;
                            // 构建请求
                            let mut request_body = json!({
                                "model": llm.model_name,
                                "messages": prepared_messages,
                            });
                            if let Some(max_tok) = max_tokens {
                                request_body["max_tokens"] = json!(max_tok);
                            }

                            // 调用 LLM
                            match call_chat_completions(
                                &llm.base_url,
                                &llm.api_key,
                                &request_body,
                                llm.max_request_bytes,
                            )
                            .await
                            {
                                Ok(content) => (true, content),
                                Err(e) => (false, e.to_string()),
                            }
                        }
                        Err(e) => (false, e),
                    };

                // 回调插件
                match state
                    .plugin_manager
                    .on_llm_response(plugin_id, request_id, success, &content)
                    .await
                {
                    Ok(new_outputs) => {
                        // 递归处理回调产生的新输出
                        Box::pin(process_plugin_outputs_with_llm_response(
                            state,
                            runtime,
                            bot_id,
                            plugin_id,
                            &new_outputs,
                        ))
                        .await;
                    }
                    Err(e) => {
                        warn!("[{}] 插件 {} onLlmResponse 失败: {}", bot_id, plugin_id, e);
                    }
                }
            }
            PluginOutput::CallLlmChatWithSearch {
                request_id,
                model_name,
                messages,
                max_tokens,
                enable_search,
            } => {
                // 解析 LLM 配置（优先使用 websearch 模型）
                let model_to_use = model_name.as_deref().or(Some("websearch"));
                let tavily_key = get_tavily_api_key(state, bot_id);
                let (success, content) =
                    match resolve_llm_config_by_name(state, bot_id, model_to_use) {
                        Ok(llm) => {
                            let mut prepared_messages = messages.clone();
                            let _ = inline_multimodal_media_in_messages(
                                &mut prepared_messages,
                                30_000,
                                15_000_000,
                                1024,
                                1024,
                                80,
                                600_000,
                                2,
                            )
                            .await;
                            // 构建请求
                            let mut request_body = json!({
                                "model": llm.model_name,
                                "messages": prepared_messages,
                            });
                            if let Some(max_tok) = max_tokens {
                                request_body["max_tokens"] = json!(max_tok);
                            }

                            // 调用支持搜索的 LLM（使用 Tavily 函数调用）
                            let search_enabled = enable_search.unwrap_or(true);
                            match call_chat_completions_with_tavily(
                                &llm.base_url,
                                &llm.api_key,
                                &request_body,
                                llm.max_request_bytes,
                                search_enabled,
                                tavily_key.as_deref(),
                            )
                            .await
                            {
                                Ok(content) => (true, content),
                                Err(e) => (false, e.to_string()),
                            }
                        }
                        Err(e) => (false, e),
                    };

                // 回调插件
                match state
                    .plugin_manager
                    .on_llm_response(plugin_id, request_id, success, &content)
                    .await
                {
                    Ok(new_outputs) => {
                        Box::pin(process_plugin_outputs_with_llm_response(
                            state,
                            runtime,
                            bot_id,
                            plugin_id,
                            &new_outputs,
                        ))
                        .await;
                    }
                    Err(e) => {
                        warn!("[{}] 插件 {} onLlmResponse 失败: {}", bot_id, plugin_id, e);
                    }
                }
            }
            // Group info fetch outputs
            PluginOutput::FetchGroupNotice {
                request_id,
                group_id,
            } => {
                process_group_info_request(
                    state,
                    runtime,
                    bot_id,
                    plugin_id,
                    request_id,
                    "notice",
                    "_get_group_notice",
                    json!({ "group_id": group_id }),
                )
                .await;
            }
            PluginOutput::FetchGroupMsgHistory {
                request_id,
                group_id,
                count,
                message_seq,
            } => {
                let mut params = json!({ "group_id": group_id });
                if let Some(c) = count {
                    params["count"] = json!(c);
                }
                if let Some(seq) = message_seq {
                    params["message_seq"] = json!(seq);
                }
                process_group_info_request(
                    state,
                    runtime,
                    bot_id,
                    plugin_id,
                    request_id,
                    "msg_history",
                    "get_group_msg_history",
                    params,
                )
                .await;
            }
            PluginOutput::FetchGroupFiles {
                request_id,
                group_id,
                folder_id,
            } => {
                let action = if folder_id.is_some() {
                    "get_group_files_by_folder"
                } else {
                    "get_group_root_files"
                };
                let mut params = json!({ "group_id": group_id });
                if let Some(fid) = folder_id {
                    params["folder_id"] = json!(fid);
                }
                process_group_info_request(
                    state, runtime, bot_id, plugin_id, request_id, "files", action, params,
                )
                .await;
            }
            PluginOutput::FetchGroupFileUrl {
                request_id,
                group_id,
                file_id,
                busid,
            } => {
                let mut params = json!({
                    "group_id": group_id,
                    "file_id": file_id,
                });
                if let Some(b) = busid {
                    params["busid"] = json!(b);
                }
                process_group_info_request(
                    state,
                    runtime,
                    bot_id,
                    plugin_id,
                    request_id,
                    "file_url",
                    "get_group_file_url",
                    params,
                )
                .await;
            }
            PluginOutput::FetchFriendList { request_id } => {
                process_group_info_request(
                    state,
                    runtime,
                    bot_id,
                    plugin_id,
                    request_id,
                    "friend_list",
                    "get_friend_list",
                    json!({}),
                )
                .await;
            }
            PluginOutput::FetchGroupList { request_id } => {
                process_group_info_request(
                    state,
                    runtime,
                    bot_id,
                    plugin_id,
                    request_id,
                    "group_list",
                    "get_group_list",
                    json!({}),
                )
                .await;
            }
            PluginOutput::FetchGroupMemberList {
                request_id,
                group_id,
            } => {
                process_group_info_request(
                    state,
                    runtime,
                    bot_id,
                    plugin_id,
                    request_id,
                    "group_member_list",
                    "get_group_member_list",
                    json!({ "group_id": group_id }),
                )
                .await;
            }
            PluginOutput::DownloadFile {
                request_id,
                url,
                thread_count,
                headers,
            } => {
                let mut params = json!({ "url": url });
                if let Some(tc) = thread_count {
                    params["thread_count"] = json!(tc);
                }
                if let Some(h) = headers {
                    params["headers"] = json!(h);
                }
                process_group_info_request(
                    state,
                    runtime,
                    bot_id,
                    plugin_id,
                    request_id,
                    "download",
                    "download_file",
                    params,
                )
                .await;
            }
            // 其他输出类型委托给普通处理函数
            _ => {
                process_plugin_outputs(state, runtime, bot_id, std::slice::from_ref(output)).await;
            }
        }
    }
}

/// 处理带有来源插件 ID 的输出列表
/// 支持 LLM 回调
pub(super) async fn process_plugin_outputs_with_source(
    state: &SharedState,
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    outputs: &[PluginOutputWithSource],
) {
    use super::llm_forward::multimodal::common::{
        call_chat_completions, call_chat_completions_with_tavily, resolve_llm_config_by_name,
    };

    for output_with_source in outputs {
        let plugin_id = &output_with_source.plugin_id;
        let output = &output_with_source.output;

        match output {
            PluginOutput::CallLlmChat {
                request_id,
                model_name,
                messages,
                max_tokens,
            } => {
                // 解析 LLM 配置
                let (success, content) =
                    match resolve_llm_config_by_name(state, bot_id, model_name.as_deref()) {
                        Ok(llm) => {
                            // 构建请求
                            let mut request_body = json!({
                                "model": llm.model_name,
                                "messages": messages,
                            });
                            if let Some(max_tok) = max_tokens {
                                request_body["max_tokens"] = json!(max_tok);
                            }

                            // 调用 LLM
                            match call_chat_completions(
                                &llm.base_url,
                                &llm.api_key,
                                &request_body,
                                llm.max_request_bytes,
                            )
                            .await
                            {
                                Ok(content) => (true, content),
                                Err(e) => (false, e.to_string()),
                            }
                        }
                        Err(e) => (false, e),
                    };

                // 回调插件
                match state
                    .plugin_manager
                    .on_llm_response(plugin_id, request_id, success, &content)
                    .await
                {
                    Ok(new_outputs) => {
                        // 递归处理回调产生的新输出
                        Box::pin(process_plugin_outputs_with_llm_response(
                            state,
                            runtime,
                            bot_id,
                            plugin_id,
                            &new_outputs,
                        ))
                        .await;
                    }
                    Err(e) => {
                        warn!("[{}] 插件 {} onLlmResponse 失败: {}", bot_id, plugin_id, e);
                    }
                }
            }
            PluginOutput::CallLlmChatWithSearch {
                request_id,
                model_name,
                messages,
                max_tokens,
                enable_search,
            } => {
                // 解析 LLM 配置（优先使用 websearch 模型）
                let model_to_use = model_name.as_deref().or(Some("websearch"));
                let tavily_key = get_tavily_api_key(state, bot_id);
                let (success, content) =
                    match resolve_llm_config_by_name(state, bot_id, model_to_use) {
                        Ok(llm) => {
                            // 构建请求
                            let mut request_body = json!({
                                "model": llm.model_name,
                                "messages": messages,
                            });
                            if let Some(max_tok) = max_tokens {
                                request_body["max_tokens"] = json!(max_tok);
                            }

                            // 调用支持搜索的 LLM（使用 Tavily 函数调用）
                            let search_enabled = enable_search.unwrap_or(true);
                            match call_chat_completions_with_tavily(
                                &llm.base_url,
                                &llm.api_key,
                                &request_body,
                                llm.max_request_bytes,
                                search_enabled,
                                tavily_key.as_deref(),
                            )
                            .await
                            {
                                Ok(content) => (true, content),
                                Err(e) => (false, e.to_string()),
                            }
                        }
                        Err(e) => (false, e),
                    };

                // 回调插件
                match state
                    .plugin_manager
                    .on_llm_response(plugin_id, request_id, success, &content)
                    .await
                {
                    Ok(new_outputs) => {
                        Box::pin(process_plugin_outputs_with_llm_response(
                            state,
                            runtime,
                            bot_id,
                            plugin_id,
                            &new_outputs,
                        ))
                        .await;
                    }
                    Err(e) => {
                        warn!("[{}] 插件 {} onLlmResponse 失败: {}", bot_id, plugin_id, e);
                    }
                }
            }
            // Group info fetch outputs
            PluginOutput::FetchGroupNotice {
                request_id,
                group_id,
            } => {
                process_group_info_request(
                    state,
                    runtime,
                    bot_id,
                    plugin_id,
                    request_id,
                    "notice",
                    "_get_group_notice",
                    json!({ "group_id": group_id }),
                )
                .await;
            }
            PluginOutput::FetchGroupMsgHistory {
                request_id,
                group_id,
                count,
                message_seq,
            } => {
                let mut params = json!({ "group_id": group_id });
                if let Some(c) = count {
                    params["count"] = json!(c);
                }
                if let Some(seq) = message_seq {
                    params["message_seq"] = json!(seq);
                }
                process_group_info_request(
                    state,
                    runtime,
                    bot_id,
                    plugin_id,
                    request_id,
                    "msg_history",
                    "get_group_msg_history",
                    params,
                )
                .await;
            }
            PluginOutput::FetchGroupFiles {
                request_id,
                group_id,
                folder_id,
            } => {
                let action = if folder_id.is_some() {
                    "get_group_files_by_folder"
                } else {
                    "get_group_root_files"
                };
                let mut params = json!({ "group_id": group_id });
                if let Some(fid) = folder_id {
                    params["folder_id"] = json!(fid);
                }
                process_group_info_request(
                    state, runtime, bot_id, plugin_id, request_id, "files", action, params,
                )
                .await;
            }
            PluginOutput::FetchGroupFileUrl {
                request_id,
                group_id,
                file_id,
                busid,
            } => {
                let mut params = json!({
                    "group_id": group_id,
                    "file_id": file_id,
                });
                if let Some(b) = busid {
                    params["busid"] = json!(b);
                }
                process_group_info_request(
                    state,
                    runtime,
                    bot_id,
                    plugin_id,
                    request_id,
                    "file_url",
                    "get_group_file_url",
                    params,
                )
                .await;
            }
            PluginOutput::DownloadFile {
                request_id,
                url,
                thread_count,
                headers,
            } => {
                let mut params = json!({ "url": url });
                if let Some(tc) = thread_count {
                    params["thread_count"] = json!(tc);
                }
                if let Some(h) = headers {
                    params["headers"] = json!(h);
                }
                process_group_info_request(
                    state,
                    runtime,
                    bot_id,
                    plugin_id,
                    request_id,
                    "download",
                    "download_file",
                    params,
                )
                .await;
            }
            // 其他输出类型委托给普通处理函数
            _ => {
                process_plugin_outputs(state, runtime, bot_id, std::slice::from_ref(output)).await;
            }
        }
    }
}

/// Helper function to process group info requests
async fn process_group_info_request(
    state: &SharedState,
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    plugin_id: &str,
    request_id: &str,
    info_type: &str,
    action: &str,
    params: serde_json::Value,
) {
    // Call the API
    let result = runtime.call_api(bot_id, action, params).await;

    let (success, data) = match result {
        Some(resp) => {
            if resp.get("status").and_then(|s| s.as_str()) == Some("ok") {
                let data = resp.get("data").cloned().unwrap_or(json!(null));
                (
                    true,
                    serde_json::to_string(&data).unwrap_or_else(|_| "null".to_string()),
                )
            } else {
                let msg = resp
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("Unknown error");
                (false, msg.to_string())
            }
        }
        None => (false, "API call failed".to_string()),
    };

    // Callback to plugin
    match state
        .plugin_manager
        .on_group_info_response(plugin_id, request_id, info_type, success, &data)
        .await
    {
        Ok(new_outputs) => {
            // Recursively process new outputs
            Box::pin(process_plugin_outputs_with_llm_response(
                state,
                runtime,
                bot_id,
                plugin_id,
                &new_outputs,
            ))
            .await;
        }
        Err(e) => {
            warn!(
                "[{}] Plugin {} onGroupInfoResponse failed: {}",
                bot_id, plugin_id, e
            );
        }
    }
}
