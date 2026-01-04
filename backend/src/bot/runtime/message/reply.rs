use serde_json::{json, Value};
use std::sync::Arc;

use crate::bot::runtime::BotRuntime;

const FORWARD_TEXT_MAX_CHARS: usize = 50_000;
const FORWARD_MAX_DEPTH: usize = 3;
const FORWARD_MEDIA_MAX_ITEMS: usize = 20;

mod forward;

/// 获取被回复消息的内容（如果有回复）
pub(super) async fn get_reply_message_content(
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    group_id: Option<u64>,
    event: &Value,
) -> Option<Value> {
    let raw_event_message = event
        .get("raw_message")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let reply_id: u64 = if let Some(message) = event.get("message").and_then(|m| m.as_array()) {
        // 从消息中查找 reply 段；若数组格式里缺失 reply，则回退到 raw_message 解析。
        let reply_seg = message
            .iter()
            .find(|seg| seg.get("type").and_then(|t| t.as_str()) == Some("reply"));

        if let Some(reply_seg) = reply_seg {
            let reply_id_val = reply_seg
                .get("data")
                .and_then(|d| d.get("id").or_else(|| d.get("message_id")))?;
            match reply_id_val {
                Value::String(s) => s.trim().parse().ok()?,
                Value::Number(n) => n.as_u64()?,
                _ => super::parse_reply_id_from_raw(raw_event_message)?,
            }
        } else {
            super::parse_reply_id_from_raw(raw_event_message)?
        }
    } else {
        super::parse_reply_id_from_raw(raw_event_message)?
    };

    // 调用 get_msg API 获取被回复消息
    let msg_data = runtime
        .call_api(bot_id, "get_msg", json!({ "message_id": reply_id }))
        .await?;

    let msg = msg_data.get("data").unwrap_or(&msg_data);

    // Sender info (used to prevent analyzing bot's own messages).
    let sender = msg.get("sender").unwrap_or(&Value::Null);
    let sender_user_id: Option<u64> = sender
        .get("user_id")
        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse::<u64>().ok()));
    let sender_nickname: Option<String> = sender
        .get("nickname")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let sender_is_bot = match (sender_user_id, runtime.get_self_id(bot_id).await) {
        (Some(sid), Some(self_id)) => sid == self_id,
        _ => false,
    };

    // 提取消息内容
    let raw_message = msg
        .get("raw_message")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let message_arr = msg.get("message").cloned().unwrap_or(Value::Null);

    // 检查是否有附件（file/image/video/record），并提取 URL。
    let mut file_url: Option<String> = None;
    let mut file_name: Option<String> = None;
    let mut image_url: Option<String> = None;
    let mut image_name: Option<String> = None;
    let mut video_url: Option<String> = None;
    let mut video_name: Option<String> = None;
    let mut record_url: Option<String> = None;
    let mut record_name: Option<String> = None;
    let mut record_file: Option<String> = None;

    if let Some(segments) = message_arr.as_array() {
        // Prefer file > video > record > image (for reply-to-media analysis).
        if let Some(seg) = segments
            .iter()
            .find(|s| s.get("type").and_then(|t| t.as_str()) == Some("file"))
        {
            if let Some(data) = seg.get("data") {
                file_name = data
                    .get("name")
                    .or_else(|| data.get("file"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                file_url = data
                    .get("url")
                    .and_then(|v| v.as_str())
                    .map(super::decode_basic_html_entities);

                let file_id =
                    data.get("file_id")
                        .or_else(|| data.get("id"))
                        .and_then(|v| match v {
                            Value::String(s) => Some(s.to_string()),
                            Value::Number(n) => n.as_u64().map(|x| x.to_string()),
                            _ => None,
                        });

                if file_url.is_none() {
                    if let (Some(fid), Some(gid)) = (file_id.as_deref(), group_id) {
                        if let Some(url_data) = runtime
                            .call_api(
                                bot_id,
                                "get_group_file_url",
                                json!({ "group_id": gid, "file_id": fid }),
                            )
                            .await
                        {
                            let url_obj = url_data.get("data").unwrap_or(&url_data);
                            file_url = url_obj
                                .get("url")
                                .and_then(|v| v.as_str())
                                .map(super::decode_basic_html_entities);
                        }
                    }
                }
            }
        } else if let Some(seg) = segments
            .iter()
            .find(|s| s.get("type").and_then(|t| t.as_str()) == Some("video"))
        {
            if let Some(data) = seg.get("data") {
                video_name = data
                    .get("file")
                    .or_else(|| data.get("name"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                video_url = data
                    .get("url")
                    .and_then(|v| v.as_str())
                    .map(super::decode_basic_html_entities);
            }
        } else if let Some(seg) = segments
            .iter()
            .find(|s| s.get("type").and_then(|t| t.as_str()) == Some("record"))
        {
            if let Some(data) = seg.get("data") {
                record_file = data
                    .get("file")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                record_name = data
                    .get("name")
                    .or_else(|| data.get("file"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                record_url = data
                    .get("url")
                    .and_then(|v| v.as_str())
                    .map(super::decode_basic_html_entities);
            }
        } else if let Some(seg) = segments
            .iter()
            .find(|s| s.get("type").and_then(|t| t.as_str()) == Some("image"))
        {
            if let Some(data) = seg.get("data") {
                image_name = data
                    .get("file")
                    .or_else(|| data.get("name"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                image_url = data
                    .get("url")
                    .and_then(|v| v.as_str())
                    .map(super::decode_basic_html_entities);
            }
        }
    }

    // Last resort: parse CQ segments from raw_message for url/file.
    if file_url.is_none()
        && file_name.is_none()
        && image_url.is_none()
        && image_name.is_none()
        && video_url.is_none()
        && video_name.is_none()
        && record_url.is_none()
        && record_name.is_none()
        && record_file.is_none()
    {
        if raw_message.contains("[CQ:file") {
            file_url = super::parse_cq_field(raw_message, "url");
            file_name = super::parse_cq_field(raw_message, "file");
        } else if raw_message.contains("[CQ:video") {
            video_url = super::parse_cq_field(raw_message, "url");
            video_name = super::parse_cq_field(raw_message, "file");
        } else if raw_message.contains("[CQ:record") {
            record_url = super::parse_cq_field(raw_message, "url");
            record_name = super::parse_cq_field(raw_message, "file");
            record_file = record_name.clone();
        } else if raw_message.contains("[CQ:image") {
            image_url = super::parse_cq_field(raw_message, "url");
            image_name = super::parse_cq_field(raw_message, "file");
        }
    }

    let forward_id = forward::extract_forward_id(&message_arr, raw_message);
    let mut forward_text: Option<String> = None;
    let mut forward_truncated = false;
    let mut forward_media: Option<Vec<Value>> = None;
    let mut forward_media_truncated = false;

    if let Some(fid) = forward_id.as_deref() {
        if let Some(fwd_data) = runtime
            .call_api(bot_id, "get_forward_msg", json!({ "message_id": fid }))
            .await
        {
            let data = fwd_data.get("data").unwrap_or(&fwd_data);
            if let Some(msgs) = data.get("messages").and_then(|v| v.as_array()) {
                let (txt, truncated) = forward::render_forward_messages(
                    msgs,
                    FORWARD_TEXT_MAX_CHARS,
                    0,
                    FORWARD_MAX_DEPTH,
                );
                forward_text = Some(txt);
                forward_truncated = truncated;

                let (media, media_truncated) = forward::collect_forward_media(msgs);
                forward_media = Some(media);
                forward_media_truncated = media_truncated;
            }
        }
    }

    Some(json!({
        "message_id": reply_id,
        "raw_message": raw_message,
        "message": message_arr,
        "sender_nickname": sender_nickname,
        "sender_is_bot": sender_is_bot,
        "file_url": file_url,
        "file_name": file_name,
        "image_url": image_url,
        "image_name": image_name,
        "video_url": video_url,
        "video_name": video_name,
        "record_url": record_url,
        "record_name": record_name,
        "record_file": record_file,
        "forward_id": forward_id,
        "forward_text": forward_text,
        "forward_truncated": forward_truncated,
        "forward_media": forward_media,
        "forward_media_truncated": forward_media_truncated,
    }))
}
