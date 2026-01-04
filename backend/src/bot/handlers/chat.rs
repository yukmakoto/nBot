use super::super::{BotRuntime, GroupSendStatus};
use axum::extract::{Json, Query};
use axum::Extension;

#[derive(serde::Deserialize)]
pub struct ChatHistoryQuery {
    pub bot_id: String,
    pub user_id: Option<u64>,
    pub group_id: Option<u64>,
    #[serde(default = "default_count")]
    pub count: u32,
}

fn default_count() -> u32 {
    50
}

#[derive(serde::Serialize)]
pub struct ChatMessage {
    pub message_id: i64,
    pub time: u64,
    pub sender_id: u64,
    pub sender_name: String,
    pub segments: Vec<ChatSegment>,
    pub is_self: bool,
}

#[derive(serde::Serialize)]
#[serde(tag = "type")]
pub enum ChatSegment {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { url: String },
    #[serde(rename = "face")]
    Face,
    #[serde(rename = "at")]
    At { qq: String },
    #[serde(rename = "reply")]
    Reply,
}

pub async fn get_chat_history_handler(
    Extension(runtime): Extension<std::sync::Arc<BotRuntime>>,
    Query(query): Query<ChatHistoryQuery>,
) -> Json<serde_json::Value> {
    // Get bot's own QQ ID for is_self detection
    let login_info = runtime
        .call_api(&query.bot_id, "get_login_info", serde_json::json!({}))
        .await;
    let self_id = login_info
        .as_ref()
        .and_then(|r| r.get("data"))
        .and_then(|d| d.get("user_id"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let (action, params) = if let Some(group_id) = query.group_id {
        (
            "get_group_msg_history",
            serde_json::json!({
                "group_id": group_id,
                "count": query.count
            }),
        )
    } else if let Some(user_id) = query.user_id {
        (
            "get_friend_msg_history",
            serde_json::json!({
                "user_id": user_id,
                "count": query.count
            }),
        )
    } else {
        return Json(
            serde_json::json!({ "status": "error", "message": "Missing user_id or group_id" }),
        );
    };

    let result = runtime.call_api(&query.bot_id, action, params).await;

    match result {
        Some(resp) => {
            if let Some(messages) = resp
                .get("data")
                .and_then(|d| d.get("messages"))
                .and_then(|m| m.as_array())
            {
                let chat_messages: Vec<ChatMessage> = messages
                    .iter()
                    .filter_map(|m| {
                        let sender = m.get("sender")?;
                        let sender_id = sender.get("user_id").and_then(|v| v.as_u64())?;

                        let segments = if let Some(arr) =
                            m.get("message").and_then(|msg| msg.as_array())
                        {
                            arr.iter()
                                .filter_map(|seg| {
                                    let seg_type = seg.get("type").and_then(|t| t.as_str())?;
                                    let data =
                                        seg.get("data").cloned().unwrap_or(serde_json::json!({}));
                                    match seg_type {
                                        "text" => {
                                            data.get("text").and_then(|t| t.as_str()).map(|text| {
                                                ChatSegment::Text {
                                                    text: text.to_string(),
                                                }
                                            })
                                        }
                                        "image" => data
                                            .get("url")
                                            .and_then(|u| u.as_str())
                                            .and_then(|url| {
                                                let url = url.trim();
                                                if url.starts_with("http://")
                                                    || url.starts_with("https://")
                                                {
                                                    Some(ChatSegment::Image {
                                                        url: url.to_string(),
                                                    })
                                                } else {
                                                    None
                                                }
                                            }),
                                        "face" => Some(ChatSegment::Face),
                                        "at" => Some(ChatSegment::At {
                                            qq: data
                                                .get("qq")
                                                .and_then(|q| q.as_str())
                                                .unwrap_or("all")
                                                .to_string(),
                                        }),
                                        "reply" => Some(ChatSegment::Reply),
                                        _ => None,
                                    }
                                })
                                .collect::<Vec<_>>()
                        } else if let Some(raw) = m.get("raw_message").and_then(|r| r.as_str()) {
                            vec![ChatSegment::Text {
                                text: raw.to_string(),
                            }]
                        } else {
                            vec![]
                        };

                        Some(ChatMessage {
                            message_id: m.get("message_id").and_then(|v| v.as_i64()).unwrap_or(0),
                            time: m.get("time").and_then(|v| v.as_u64()).unwrap_or(0),
                            sender_id,
                            sender_name: sender
                                .get("nickname")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            segments,
                            is_self: sender_id == self_id,
                        })
                    })
                    .collect();

                Json(serde_json::json!({ "status": "success", "messages": chat_messages }))
            } else {
                let empty: Vec<ChatMessage> = vec![];
                Json(serde_json::json!({ "status": "success", "messages": empty }))
            }
        }
        None => {
            Json(serde_json::json!({ "status": "error", "message": "Failed to get chat history" }))
        }
    }
}

#[derive(serde::Deserialize)]
pub struct SendMessagePayload {
    pub bot_id: String,
    pub user_id: Option<u64>,
    pub group_id: Option<u64>,
    pub message: String,
}

pub async fn send_chat_message_handler(
    Extension(runtime): Extension<std::sync::Arc<BotRuntime>>,
    Json(payload): Json<SendMessagePayload>,
) -> Json<serde_json::Value> {
    let (action, params) = if let Some(group_id) = payload.group_id {
        if matches!(
            runtime
                .get_group_send_status(&payload.bot_id, group_id)
                .await,
            GroupSendStatus::Muted
        ) {
            return Json(
                serde_json::json!({ "status": "error", "message": "Bot is muted in the target group" }),
            );
        }
        (
            "send_group_msg",
            serde_json::json!({
                "group_id": group_id,
                "message": payload.message
            }),
        )
    } else if let Some(user_id) = payload.user_id {
        (
            "send_private_msg",
            serde_json::json!({
                "user_id": user_id,
                "message": payload.message
            }),
        )
    } else {
        return Json(
            serde_json::json!({ "status": "error", "message": "Missing user_id or group_id" }),
        );
    };

    let result = runtime.call_api(&payload.bot_id, action, params).await;

    match result {
        Some(resp) => {
            if resp.get("status").and_then(|s| s.as_str()) == Some("ok") {
                Json(serde_json::json!({ "status": "success" }))
            } else {
                let msg = resp
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("Unknown error");
                Json(serde_json::json!({ "status": "error", "message": msg }))
            }
        }
        None => Json(serde_json::json!({ "status": "error", "message": "Failed to send message" })),
    }
}
