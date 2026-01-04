use crate::models::SharedState;
use crate::persistence::save_bots;
use futures_util::{SinkExt, StreamExt};
use reqwest::Client as HttpClient;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::watch;
use tokio::time::{sleep, Duration};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::protocol::Message;
use tracing::{error, info, warn};

use super::connection::{BotConnection, BotRuntime, DiscordConnection};
use super::message::handle_event;

const DISCORD_GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";

// Intents:
// - GUILDS (1<<0): needed for READY + basic guild context
// - GUILD_MESSAGES (1<<9), DIRECT_MESSAGES (1<<12)
// - MESSAGE_CONTENT (1<<15): privileged, required to read content
const DISCORD_INTENTS: u64 = (1 << 0) | (1 << 9) | (1 << 12) | (1 << 15);

fn get_discord_token_from_bot(bot: &crate::models::BotInstance) -> Option<String> {
    bot.metadata
        .get("discord")
        .and_then(|v| v.get("token"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn normalize_discord_token(token: &str) -> String {
    token
        .trim()
        .strip_prefix("Bot ")
        .unwrap_or(token.trim())
        .to_string()
}

fn parse_u64_field(v: Option<&Value>) -> Option<u64> {
    match v? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse::<u64>().ok(),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct DiscordResumeState {
    session_id: String,
    resume_gateway_url: String,
    seq: u64,
}

fn best_author_name(msg: &Value) -> String {
    msg.get("member")
        .and_then(|m| m.get("nick"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
        .or_else(|| {
            msg.get("author")
                .and_then(|a| a.get("global_name"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.to_string())
        })
        .or_else(|| {
            msg.get("author")
                .and_then(|a| a.get("username"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn attachment_segment(att: &Value) -> Option<Value> {
    let url = att.get("url").and_then(|v| v.as_str())?;
    let filename = att
        .get("filename")
        .and_then(|v| v.as_str())
        .unwrap_or("file");
    let size = att.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
    let content_type = att
        .get("content_type")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let ty = if content_type.starts_with("image/") {
        "image"
    } else if content_type.starts_with("video/") {
        "video"
    } else if content_type.starts_with("audio/") {
        "record"
    } else {
        "file"
    };

    Some(json!({
        "type": ty,
        "data": {
            "url": url,
            "file": filename,
            "name": filename,
            "size": size,
        }
    }))
}

fn referenced_message_id(msg: &Value) -> Option<u64> {
    parse_u64_field(
        msg.get("referenced_message")
            .and_then(|m| m.get("id"))
            .or_else(|| {
                msg.get("message_reference")
                    .and_then(|m| m.get("message_id"))
            }),
    )
}

fn build_onebot_like_event(bot_id: &str, msg: &Value) -> Option<Value> {
    let channel_id = parse_u64_field(msg.get("channel_id"))?;
    let is_guild = msg.get("guild_id").map(|v| !v.is_null()).unwrap_or(false);
    let message_type = if is_guild { "group" } else { "private" };

    let user_id = parse_u64_field(msg.get("author").and_then(|a| a.get("id")))?;
    let user_id_str = user_id.to_string();
    let group_id_str = is_guild.then(|| channel_id.to_string());

    let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");

    let mut segments: Vec<Value> = Vec::new();
    if let Some(reply_id) = referenced_message_id(msg) {
        segments.push(json!({ "type": "reply", "data": { "id": reply_id.to_string() } }));
    }
    if !content.is_empty() {
        segments.push(json!({ "type": "text", "data": { "text": content } }));
    }

    if let Some(atts) = msg.get("attachments").and_then(|v| v.as_array()) {
        for att in atts {
            if let Some(seg) = attachment_segment(att) {
                segments.push(seg);
            }
        }
    }

    Some(json!({
        "post_type": "message",
        "message_type": message_type,
        // Use string IDs to avoid JS precision loss in plugins.
        "user_id": user_id_str,
        "group_id": group_id_str,
        "raw_message": content,
        "message": segments,
        "platform": "Discord",
        "bot_id": bot_id,
        "discord": {
            "channel_id": channel_id.to_string(),
            "guild_id": msg.get("guild_id").cloned().unwrap_or(Value::Null),
            "message_id": msg.get("id").cloned().unwrap_or(Value::Null),
        }
    }))
}

fn build_indexed_msg_data(msg: &Value) -> Option<(u64, Value)> {
    let message_id = parse_u64_field(msg.get("id"))?;
    let sender_name = best_author_name(msg);

    let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");

    let mut segments: Vec<Value> = Vec::new();
    if !content.is_empty() {
        segments.push(json!({ "type": "text", "data": { "text": content } }));
    }
    if let Some(atts) = msg.get("attachments").and_then(|v| v.as_array()) {
        for att in atts {
            if let Some(seg) = attachment_segment(att) {
                segments.push(seg);
            }
        }
    }

    let user_id = parse_u64_field(msg.get("author").and_then(|a| a.get("id")))?;

    Some((
        message_id,
        json!({
            "message_id": message_id.to_string(),
            "raw_message": content,
            "message": segments,
            "sender": {
                "user_id": user_id.to_string(),
                "nickname": sender_name,
            }
        }),
    ))
}

enum DiscordExit {
    Shutdown,
    Reconnect { resume: Option<DiscordResumeState> },
}

async fn discord_connect_and_run(
    state: SharedState,
    runtime: Arc<BotRuntime>,
    bot_id: String,
    token: Arc<String>,
    resume: Option<DiscordResumeState>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<DiscordExit, String> {
    let gateway_url = resume
        .as_ref()
        .map(|s| s.resume_gateway_url.as_str())
        .unwrap_or(DISCORD_GATEWAY_URL);
    let (ws_stream, _resp) = connect_async(gateway_url)
        .await
        .map_err(|e| format!("connect gateway failed: {e}"))?;
    let (mut write, mut read) = ws_stream.split();

    let mut seq: Option<u64> = None;
    let mut session_id: Option<String> = resume.as_ref().map(|s| s.session_id.clone());
    let mut resume_url: Option<String> = resume.as_ref().map(|s| s.resume_gateway_url.clone());

    // Wait HELLO
    let heartbeat_interval_ms: u64 = loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    return Ok(DiscordExit::Shutdown);
                }
            }
            msg = read.next() => {
                let msg = msg.ok_or_else(|| "gateway closed before hello".to_string())?;
                let text = match msg.map_err(|e| format!("gateway read error: {e}"))? {
                    Message::Text(t) => t,
                    Message::Binary(b) => String::from_utf8(b).map_err(|e| format!("gateway binary utf8 error: {e}"))?,
                    _ => continue,
                };
                let payload: Value = serde_json::from_str(&text).map_err(|e| format!("gateway json error: {e}"))?;
                if let Some(s) = payload.get("s").and_then(|v| v.as_u64()) {
                    seq = Some(s);
                }
                let op = payload.get("op").and_then(|v| v.as_i64()).unwrap_or(-1);
                if op == 10 {
                    let interval = payload.get("d")
                        .and_then(|d| d.get("heartbeat_interval"))
                        .and_then(|v| v.as_u64())
                        .ok_or_else(|| "missing heartbeat_interval".to_string())?;
                    break interval;
                }
            }
        }
    };

    // Identify / Resume
    if let (Some(sid), Some(seq_num)) = (session_id.as_deref(), resume.as_ref().map(|s| s.seq)) {
        let resume_payload = json!({
            "op": 6,
            "d": {
                "token": normalize_discord_token(&token),
                "session_id": sid,
                "seq": seq_num,
            }
        });
        write
            .send(Message::Text(resume_payload.to_string()))
            .await
            .map_err(|e| format!("send resume failed: {e}"))?;
    } else {
        let identify = json!({
            "op": 2,
            "d": {
                "token": normalize_discord_token(&token),
                "intents": DISCORD_INTENTS,
                "properties": {
                    "os": std::env::consts::OS,
                    "browser": "nBot",
                    "device": "nBot"
                }
            }
        });
        write
            .send(Message::Text(identify.to_string()))
            .await
            .map_err(|e| format!("send identify failed: {e}"))?;
    }

    let mut hb = tokio::time::interval(Duration::from_millis(heartbeat_interval_ms));
    hb.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    break;
                }
            }
            _ = hb.tick() => {
                let heartbeat = json!({ "op": 1, "d": seq });
                if write.send(Message::Text(heartbeat.to_string())).await.is_err() {
                    return Err("send heartbeat failed".to_string());
                }
            }
            msg = read.next() => {
                let Some(msg) = msg else {
                    return Err("gateway closed".to_string());
                };
                let msg = msg.map_err(|e| format!("gateway read error: {e}"))?;
                let text = match msg {
                    Message::Text(t) => t,
                    Message::Binary(b) => String::from_utf8(b).map_err(|e| format!("gateway binary utf8 error: {e}"))?,
                    Message::Close(_) => return Err("gateway closed".to_string()),
                    _ => continue,
                };
                let payload: Value = serde_json::from_str(&text).map_err(|e| format!("gateway json error: {e}"))?;

                if let Some(s) = payload.get("s").and_then(|v| v.as_u64()) {
                    seq = Some(s);
                }

                let op = payload.get("op").and_then(|v| v.as_i64()).unwrap_or(-1);
                match op {
                    0 => {
                        // DISPATCH
                        let t = payload.get("t").and_then(|v| v.as_str()).unwrap_or("");
                        let d = payload.get("d").cloned().unwrap_or(Value::Null);

                        match t {
                            "READY" => {
                                let self_id = parse_u64_field(d.get("user").and_then(|u| u.get("id"))).unwrap_or(0);
                                if self_id != 0 {
                                    runtime.set_self_id(&bot_id, self_id).await;
                                }
                                session_id = d.get("session_id").and_then(|v| v.as_str()).map(|s| s.to_string());
                                resume_url = d.get("resume_gateway_url").and_then(|v| v.as_str()).map(|s| s.to_string());

                                if let Some(mut bot) = state.bots.get_mut(&bot_id) {
                                    bot.is_connected = true;
                                    bot.metadata["discord"]["bot_user_id"] = json!(self_id.to_string());
                                }
                                save_bots(&state.bots);
                                info!("[{}] Discord 已连接", bot_id);
                            }
                            "MESSAGE_CREATE" => {
                                // Ignore bot users (including ourselves).
                                let author_bot = d.get("author")
                                    .and_then(|a| a.get("bot"))
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                if author_bot {
                                    continue;
                                }

                                if let Some((message_id, data)) = build_indexed_msg_data(&d) {
                                    runtime.index_discord_message(&bot_id, message_id, data).await;
                                }

                                if let Some(event) = build_onebot_like_event(&bot_id, &d) {
                                    let state_cl = state.clone();
                                    let runtime_cl = runtime.clone();
                                    let bot_id_cl = bot_id.clone();
                                    tokio::spawn(async move {
                                        handle_event(&state_cl, &runtime_cl, &bot_id_cl, event).await;
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                    1 => {
                        // Heartbeat requested by server
                        let heartbeat = json!({ "op": 1, "d": seq });
                        if write.send(Message::Text(heartbeat.to_string())).await.is_err() {
                            return Err("send heartbeat failed".to_string());
                        }
                    }
                    7 => {
                        // RECONNECT
                        warn!("[{}] Discord gateway requested reconnect", bot_id);
                        break;
                    }
                    9 => {
                        // INVALID_SESSION
                        let can_resume = payload.get("d").and_then(|v| v.as_bool()).unwrap_or(false);
                        if !can_resume {
                            session_id = None;
                            resume_url = None;
                        }
                        warn!("[{}] Discord invalid session (can_resume={})", bot_id, can_resume);
                        break;
                    }
                    10 | 11 => {
                        // HELLO / HEARTBEAT_ACK - handled
                    }
                    _ => {}
                }
            }
        }
    }

    // Graceful close
    let _ = write.send(Message::Close(None)).await;

    let resume = match (session_id.as_deref(), resume_url.as_deref(), seq) {
        (Some(sid), Some(url), Some(seq)) => Some(DiscordResumeState {
            session_id: sid.to_string(),
            resume_gateway_url: format!("{url}?v=10&encoding=json"),
            seq,
        }),
        _ => None,
    };

    if *shutdown_rx.borrow() {
        Ok(DiscordExit::Shutdown)
    } else {
        Ok(DiscordExit::Reconnect { resume })
    }
}

async fn start_discord_bot(
    state: SharedState,
    runtime: Arc<BotRuntime>,
    bot_id: String,
    token: String,
) -> Result<(), String> {
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    let token = Arc::new(normalize_discord_token(&token));

    let http = HttpClient::builder()
        .user_agent("nBot (https://github.com/; discord backend)")
        .build()
        .map_err(|e| format!("build http client failed: {e}"))?;

    runtime
        .register_discord_connection(
            &bot_id,
            DiscordConnection {
                token: token.clone(),
                http,
                shutdown: shutdown_tx,
            },
        )
        .await;

    // Persist desired running state.
    if let Some(mut bot) = state.bots.get_mut(&bot_id) {
        bot.is_running = true;
    }
    save_bots(&state.bots);

    tokio::spawn(async move {
        let mut backoff = Duration::from_secs(1);
        let mut resume: Option<DiscordResumeState> = None;
        loop {
            if *shutdown_rx.borrow() {
                break;
            }

            let run_state = state.clone();
            let run_runtime = runtime.clone();
            let run_bot_id = bot_id.clone();
            let run_token = token.clone();
            let run_resume = resume.clone();
            let run_shutdown = shutdown_rx.clone();

            match discord_connect_and_run(
                run_state,
                run_runtime,
                run_bot_id,
                run_token,
                run_resume,
                run_shutdown,
            )
            .await
            {
                Ok(DiscordExit::Shutdown) => break,
                Ok(DiscordExit::Reconnect { resume: r }) => {
                    resume = r;
                }
                Err(e) => {
                    warn!("[{}] Discord gateway error: {}", bot_id, e);
                }
            };

            if *shutdown_rx.borrow() {
                break;
            }

            // Mark disconnected (but keep is_running=true for reconnect attempts).
            if let Some(mut bot) = state.bots.get_mut(&bot_id) {
                if bot.is_connected {
                    bot.is_connected = false;
                    save_bots(&state.bots);
                }
            }

            tokio::select! {
                _ = sleep(backoff) => {},
                _ = shutdown_rx.changed() => {},
            }
            backoff = (backoff * 2).min(Duration::from_secs(30));
        }

        // Final cleanup
        runtime.unregister_connection(&bot_id).await;
        if let Some(mut bot) = state.bots.get_mut(&bot_id) {
            bot.is_connected = false;
        }
        save_bots(&state.bots);
        info!("[{}] Discord 已停止", bot_id);
    });

    Ok(())
}

async fn stop_discord_bot(state: &SharedState, runtime: &Arc<BotRuntime>, bot_id: &str) {
    let conn = {
        let conns = runtime.connections.read().await;
        match conns.get(bot_id).cloned() {
            Some(BotConnection::Discord(c)) => Some(c),
            _ => None,
        }
    };

    let Some(conn) = conn else {
        return;
    };

    let _ = conn.shutdown.send(true);
    runtime.unregister_connection(bot_id).await;

    if let Some(mut bot) = state.bots.get_mut(bot_id) {
        bot.is_connected = false;
        bot.is_running = false;
    }
    save_bots(&state.bots);
}

pub async fn start_discord_connections(state: SharedState, runtime: Arc<BotRuntime>) {
    info!("启动 Discord 连接管理循环...");

    loop {
        let to_start: Vec<(String, String)> = state
            .bots
            .iter()
            .filter(|b| b.platform.eq_ignore_ascii_case("discord"))
            .filter(|b| b.is_running)
            .filter_map(|b| {
                let token = get_discord_token_from_bot(b.value())?;
                Some((b.id.clone(), token))
            })
            .collect();

        for (bot_id, token) in to_start {
            let already = {
                let conns = runtime.connections.read().await;
                matches!(conns.get(&bot_id), Some(BotConnection::Discord(_)))
            };
            if already {
                continue;
            }

            info!("[{}] 启动 Discord Bot...", bot_id);
            if let Err(e) =
                start_discord_bot(state.clone(), runtime.clone(), bot_id.clone(), token).await
            {
                error!("[{}] 启动 Discord Bot 失败: {}", bot_id, e);
                if let Some(mut bot) = state.bots.get_mut(&bot_id) {
                    bot.is_connected = false;
                    bot.is_running = false;
                }
                save_bots(&state.bots);
            }
        }

        let to_stop: Vec<String> = {
            let conns = runtime.connections.read().await;
            state
                .bots
                .iter()
                .filter(|b| b.platform.eq_ignore_ascii_case("discord"))
                .filter(|b| !b.is_running)
                .filter_map(|b| {
                    matches!(conns.get(&b.id), Some(BotConnection::Discord(_)))
                        .then_some(b.id.clone())
                })
                .collect()
        };

        for bot_id in to_stop {
            info!("[{}] 停止 Discord Bot...", bot_id);
            stop_discord_bot(&state, &runtime, &bot_id).await;
        }

        sleep(Duration::from_secs(2)).await;
    }
}
