use base64::Engine;
use serde_json::{json, Value};
use std::cmp;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

use super::connection::{BotConnection, BotRuntime, DiscordConnection, GroupSendStatus};

const DISCORD_API_BASE: &str = "https://discord.com/api/v10";
const DISCORD_MAX_CONTENT_CHARS: usize = 2000;
const DISCORD_MAX_ATTACHMENTS: usize = 10;

/// 计算消息的去重哈希值
fn compute_message_hash(bot_id: &str, target: u64, message: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    bot_id.hash(&mut hasher);
    target.hash(&mut hasher);
    message.hash(&mut hasher);
    hasher.finish()
}

pub async fn send_reply(
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    user_id: u64,
    group_id: Option<u64>,
    message: &str,
) {
    if let Some(gid) = group_id {
        match runtime.get_group_send_status(bot_id, gid).await {
            GroupSendStatus::Muted => {
                warn!("[{}] 群 {} 内机器人被禁言，跳过发送群消息", bot_id, gid);
                return;
            }
            GroupSendStatus::Unknown | GroupSendStatus::Allowed => {}
        }

        // 去重检查：相同消息在5秒内不重复发送（仅在允许发送时才记录）
        let hash = compute_message_hash(bot_id, gid, message);
        if runtime.message_dedup.lock().await.is_duplicate(hash) {
            warn!("[{}] 消息去重: 跳过重复消息发送", bot_id);
            return;
        }

        send_api(
            runtime,
            bot_id,
            "send_group_msg",
            json!({
                "group_id": gid,
                "message": message
            }),
        )
        .await;
    } else {
        // 去重检查：相同消息在5秒内不重复发送（仅在允许发送时才记录）
        let hash = compute_message_hash(bot_id, user_id, message);
        if runtime.message_dedup.lock().await.is_duplicate(hash) {
            warn!("[{}] 消息去重: 跳过重复消息发送", bot_id);
            return;
        }

        send_api(
            runtime,
            bot_id,
            "send_private_msg",
            json!({
                "user_id": user_id,
                "message": message
            }),
        )
        .await;
    }
}

pub async fn send_api(runtime: &Arc<BotRuntime>, bot_id: &str, action: &str, params: Value) {
    if let Some(gid) = extract_group_id_for_send_action(action, &params) {
        match runtime.get_group_send_status(bot_id, gid).await {
            GroupSendStatus::Muted => {
                warn!(
                    "[{}] 群 {} 内机器人被禁言，跳过发送 {}",
                    bot_id, gid, action
                );
                return;
            }
            GroupSendStatus::Unknown | GroupSendStatus::Allowed => {}
        }
    }

    let conns = runtime.connections.read().await;
    let Some(conn) = conns.get(bot_id).cloned() else {
        warn!("[{}] 无法发送API调用，连接不存在", bot_id);
        return;
    };
    drop(conns);

    match conn {
        BotConnection::OneBot { sender: tx } => {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let msg = json!({
                "action": action,
                "params": params,
                "echo": format!("{}_{}", action, now_ms)
            });
            if tx.send(msg.to_string()).is_err() {
                warn!("[{}] 发送API调用失败: {}", bot_id, action);
            } else {
                info!("[{}] 发送API: {}", bot_id, action);
            }
        }
        BotConnection::Discord(conn) => {
            if let Err(e) = discord_send_api(runtime, bot_id, &conn, action, &params).await {
                warn!("[{}] Discord API {} 失败: {}", bot_id, action, e);
            } else {
                info!("[{}] Discord API: {}", bot_id, action);
            }
        }
    }
}

fn parse_u64(v: Option<&Value>) -> Option<u64> {
    match v? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse::<u64>().ok(),
        _ => None,
    }
}

fn extract_group_id_for_send_action(action: &str, params: &Value) -> Option<u64> {
    match action {
        "send_group_msg" | "send_group_forward_msg" => parse_u64(params.get("group_id")),
        "send_msg" => {
            let ty = params.get("message_type").and_then(|v| v.as_str())?;
            (ty == "group")
                .then(|| parse_u64(params.get("group_id")))
                .flatten()
        }
        "send_forward_msg" => parse_u64(params.get("group_id")),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct DiscordUploadFile {
    filename: String,
    bytes: Vec<u8>,
}

fn guess_image_ext(data: &[u8]) -> &'static str {
    if data.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        "png"
    } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "jpg"
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        "gif"
    } else if data.len() > 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        "webp"
    } else {
        "bin"
    }
}

fn extract_base64_cq_images(message: &str) -> (String, Vec<DiscordUploadFile>) {
    // Minimal CQ parser: extract all `[CQ:image,file=base64://...]` images and strip them from content.
    let mut content = message.to_string();
    let mut files: Vec<DiscordUploadFile> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    while let Some(start) = content.find("[CQ:image") {
        let end = match content[start..].find(']') {
            Some(i) => start + i,
            None => break,
        };

        let seg = content[start..=end].to_string();
        let file_pos = match seg.find("file=") {
            Some(p) => p + "file=".len(),
            None => {
                content.replace_range(start..=end, "");
                continue;
            }
        };
        let after = &seg[file_pos..seg.len() - 1];
        let raw_file = after.split(',').next().unwrap_or(after).trim();
        let b64 = raw_file
            .strip_prefix("base64://")
            .or_else(|| raw_file.strip_prefix("base64:"));

        if let Some(b64) = b64 {
            let b64 = b64.trim();
            if !b64.is_empty() && seen.insert(b64.to_string()) {
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
                    let ext = guess_image_ext(&bytes);
                    let filename = format!("image_{}.{}", files.len() + 1, ext);
                    files.push(DiscordUploadFile { filename, bytes });
                }
            }
        }

        content.replace_range(start..=end, "");
    }

    (content.trim().to_string(), files)
}

fn split_discord_content(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();

    for part in content.split_inclusive('\n') {
        let part_len = part.chars().count();
        let cur_len = cur.chars().count();
        if cur_len + part_len <= DISCORD_MAX_CONTENT_CHARS {
            cur.push_str(part);
            continue;
        }

        if !cur.trim().is_empty() {
            out.push(cur.trim_end_matches('\n').to_string());
            cur.clear();
        }

        if part_len <= DISCORD_MAX_CONTENT_CHARS {
            cur.push_str(part);
            continue;
        }

        // Very long single line: hard split by char count.
        let mut buf = String::new();
        for ch in part.chars() {
            buf.push(ch);
            if buf.chars().count() >= DISCORD_MAX_CONTENT_CHARS {
                out.push(buf);
                buf = String::new();
            }
        }
        cur = buf;
    }

    if !cur.trim().is_empty() {
        out.push(cur.trim_end_matches('\n').to_string());
    }

    out
}

fn discord_auth_header(token: &str) -> String {
    format!(
        "Bot {}",
        token.trim().strip_prefix("Bot ").unwrap_or(token.trim())
    )
}

fn is_discord_permission_error(status: reqwest::StatusCode, body: &str) -> bool {
    status == reqwest::StatusCode::FORBIDDEN
        || body.contains("\"code\": 50013")
        || body.contains("Missing Permissions")
}

async fn discord_post_json_with_retry(
    http: &reqwest::Client,
    token: &str,
    url: &str,
    payload: &Value,
) -> Result<(reqwest::StatusCode, String), String> {
    let auth = discord_auth_header(token);
    let mut attempts = 0u32;

    loop {
        attempts += 1;
        let resp = http
            .post(url)
            .header(reqwest::header::AUTHORIZATION, auth.clone())
            .json(payload)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {e}"))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| format!("read response failed: {e}"))?;

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            if attempts >= 6 {
                return Err(format!("rate limited too many times: {body}"));
            }
            let retry_after = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v.get("retry_after").and_then(|x| x.as_f64()))
                .unwrap_or(1.0);
            let wait_ms = cmp::max(250, (retry_after * 1000.0).ceil() as u64);
            sleep(Duration::from_millis(wait_ms)).await;
            continue;
        }

        return Ok((status, body));
    }
}

async fn discord_post_multipart_with_retry(
    http: &reqwest::Client,
    token: &str,
    url: &str,
    payload_json: &str,
    files: &[DiscordUploadFile],
) -> Result<(reqwest::StatusCode, String), String> {
    let auth = discord_auth_header(token);
    let mut attempts = 0u32;

    loop {
        attempts += 1;
        let mut form =
            reqwest::multipart::Form::new().text("payload_json", payload_json.to_string());
        for (i, f) in files.iter().enumerate() {
            let part =
                reqwest::multipart::Part::bytes(f.bytes.clone()).file_name(f.filename.clone());
            form = form.part(format!("files[{i}]"), part);
        }

        let resp = http
            .post(url)
            .header(reqwest::header::AUTHORIZATION, auth.clone())
            .multipart(form)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {e}"))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| format!("read response failed: {e}"))?;

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            if attempts >= 6 {
                return Err(format!("rate limited too many times: {body}"));
            }
            let retry_after = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v.get("retry_after").and_then(|x| x.as_f64()))
                .unwrap_or(1.0);
            let wait_ms = cmp::max(250, (retry_after * 1000.0).ceil() as u64);
            sleep(Duration::from_millis(wait_ms)).await;
            continue;
        }

        return Ok((status, body));
    }
}

async fn discord_create_dm_channel(conn: &DiscordConnection, user_id: u64) -> Result<u64, String> {
    let url = format!("{}/users/@me/channels", DISCORD_API_BASE);
    let payload = json!({ "recipient_id": user_id.to_string() });
    let (status, body) =
        discord_post_json_with_retry(&conn.http, &conn.token, &url, &payload).await?;
    if !status.is_success() {
        return Err(format!("HTTP {status}: {body}"));
    }

    let v: Value = serde_json::from_str(&body).map_err(|e| format!("parse dm response: {e}"))?;
    parse_u64(v.get("id")).ok_or_else(|| "missing dm channel id".to_string())
}

async fn discord_send_channel_message(
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    conn: &DiscordConnection,
    channel_id: u64,
    content: &str,
    files: Vec<DiscordUploadFile>,
) -> Result<(), String> {
    if content.trim().is_empty() && files.is_empty() {
        return Ok(());
    }

    let url = format!("{}/channels/{}/messages", DISCORD_API_BASE, channel_id);
    let mut chunks = split_discord_content(content);
    if chunks.is_empty() {
        chunks.push(String::new());
    }

    let mut remaining_files = files;
    let mut first = true;

    for chunk in chunks {
        if first {
            first = false;

            if remaining_files.is_empty() {
                if !chunk.trim().is_empty() {
                    let payload = json!({ "content": chunk });
                    let (status, body) =
                        discord_post_json_with_retry(&conn.http, &conn.token, &url, &payload)
                            .await?;
                    if !status.is_success() {
                        if is_discord_permission_error(status, &body) {
                            runtime
                                .cache_group_send_status(bot_id, channel_id, GroupSendStatus::Muted)
                                .await;
                        }
                        return Err(format!("HTTP {status}: {body}"));
                    }
                }
                continue;
            }

            let take = cmp::min(DISCORD_MAX_ATTACHMENTS, remaining_files.len());
            let batch: Vec<DiscordUploadFile> = remaining_files.drain(0..take).collect();

            let payload = json!({ "content": chunk });
            let payload_json = payload.to_string();
            let (status, body) = discord_post_multipart_with_retry(
                &conn.http,
                &conn.token,
                &url,
                &payload_json,
                &batch,
            )
            .await?;
            if !status.is_success() {
                if is_discord_permission_error(status, &body) {
                    runtime
                        .cache_group_send_status(bot_id, channel_id, GroupSendStatus::Muted)
                        .await;
                }
                return Err(format!("HTTP {status}: {body}"));
            }

            while !remaining_files.is_empty() {
                let take = cmp::min(DISCORD_MAX_ATTACHMENTS, remaining_files.len());
                let batch: Vec<DiscordUploadFile> = remaining_files.drain(0..take).collect();

                let payload = json!({ "content": "" });
                let payload_json = payload.to_string();
                let (status, body) = discord_post_multipart_with_retry(
                    &conn.http,
                    &conn.token,
                    &url,
                    &payload_json,
                    &batch,
                )
                .await?;
                if !status.is_success() {
                    if is_discord_permission_error(status, &body) {
                        runtime
                            .cache_group_send_status(bot_id, channel_id, GroupSendStatus::Muted)
                            .await;
                    }
                    return Err(format!("HTTP {status}: {body}"));
                }
            }

            continue;
        }

        if !chunk.trim().is_empty() {
            let payload = json!({ "content": chunk });
            let (status, body) =
                discord_post_json_with_retry(&conn.http, &conn.token, &url, &payload).await?;
            if !status.is_success() {
                if is_discord_permission_error(status, &body) {
                    runtime
                        .cache_group_send_status(bot_id, channel_id, GroupSendStatus::Muted)
                        .await;
                }
                return Err(format!("HTTP {status}: {body}"));
            }
        }
    }

    Ok(())
}

async fn discord_send_api(
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    conn: &DiscordConnection,
    action: &str,
    params: &Value,
) -> Result<(), String> {
    match action {
        "send_group_msg" => {
            let channel_id =
                parse_u64(params.get("group_id")).ok_or_else(|| "missing group_id".to_string())?;
            let message = params
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            let (content, files) = extract_base64_cq_images(message);
            discord_send_channel_message(runtime, bot_id, conn, channel_id, &content, files)
                .await?;
            Ok(())
        }
        "send_private_msg" => {
            let user_id =
                parse_u64(params.get("user_id")).ok_or_else(|| "missing user_id".to_string())?;
            let message = params
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            let dm_channel_id = discord_create_dm_channel(conn, user_id).await?;
            let (content, files) = extract_base64_cq_images(message);
            discord_send_channel_message(runtime, bot_id, conn, dm_channel_id, &content, files)
                .await?;
            Ok(())
        }
        "send_group_forward_msg" => {
            let channel_id =
                parse_u64(params.get("group_id")).ok_or_else(|| "missing group_id".to_string())?;
            let msgs = params
                .get("messages")
                .and_then(|v| v.as_array())
                .ok_or_else(|| "missing messages".to_string())?;

            for node in msgs {
                let content = node
                    .get("data")
                    .and_then(|d| d.get("content"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let (content, files) = extract_base64_cq_images(content);
                discord_send_channel_message(runtime, bot_id, conn, channel_id, &content, files)
                    .await?;
            }
            Ok(())
        }
        "send_private_forward_msg" => {
            let user_id =
                parse_u64(params.get("user_id")).ok_or_else(|| "missing user_id".to_string())?;
            let msgs = params
                .get("messages")
                .and_then(|v| v.as_array())
                .ok_or_else(|| "missing messages".to_string())?;

            let dm_channel_id = discord_create_dm_channel(conn, user_id).await?;
            for node in msgs {
                let content = node
                    .get("data")
                    .and_then(|d| d.get("content"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let (content, files) = extract_base64_cq_images(content);
                discord_send_channel_message(runtime, bot_id, conn, dm_channel_id, &content, files)
                    .await?;
            }
            Ok(())
        }
        "send_msg" => {
            let ty = params
                .get("message_type")
                .and_then(|v| v.as_str())
                .unwrap_or("private");

            let message = params
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            if ty == "group" {
                let channel_id = parse_u64(params.get("group_id"))
                    .ok_or_else(|| "missing group_id".to_string())?;
                let (content, files) = extract_base64_cq_images(message);
                discord_send_channel_message(runtime, bot_id, conn, channel_id, &content, files)
                    .await?;
            } else {
                let user_id = parse_u64(params.get("user_id"))
                    .ok_or_else(|| "missing user_id".to_string())?;
                let dm_channel_id = discord_create_dm_channel(conn, user_id).await?;
                let (content, files) = extract_base64_cq_images(message);
                discord_send_channel_message(runtime, bot_id, conn, dm_channel_id, &content, files)
                    .await?;
            }
            Ok(())
        }
        "send_forward_msg" => {
            let msgs = params
                .get("messages")
                .and_then(|v| v.as_array())
                .ok_or_else(|| "missing messages".to_string())?;

            if let Some(channel_id) = parse_u64(params.get("group_id")) {
                for node in msgs {
                    let content = node
                        .get("data")
                        .and_then(|d| d.get("content"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    let (content, files) = extract_base64_cq_images(content);
                    discord_send_channel_message(
                        runtime, bot_id, conn, channel_id, &content, files,
                    )
                    .await?;
                }
                return Ok(());
            }

            if let Some(user_id) = parse_u64(params.get("user_id")) {
                let dm_channel_id = discord_create_dm_channel(conn, user_id).await?;
                for node in msgs {
                    let content = node
                        .get("data")
                        .and_then(|d| d.get("content"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    let (content, files) = extract_base64_cq_images(content);
                    discord_send_channel_message(
                        runtime,
                        bot_id,
                        conn,
                        dm_channel_id,
                        &content,
                        files,
                    )
                    .await?;
                }
                return Ok(());
            }

            Err("missing group_id/user_id".to_string())
        }
        _ => Err(format!("unsupported action: {}", action)),
    }
}
