use base64::Engine;
use serde_json::{json, Value};
use std::cmp;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

use super::connection::{BotConnection, BotRuntime, DiscordConnection, GroupSendStatus};
use super::privacy;

const DISCORD_API_BASE: &str = "https://discord.com/api/v10";
const DISCORD_MAX_CONTENT_CHARS: usize = 2000;
const DISCORD_MAX_ATTACHMENTS: usize = 10;

async fn resolve_group_member_name(
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    group_id: u64,
    user_id: u64,
) -> Option<String> {
    let resp = runtime
        .call_api(
            bot_id,
            "get_group_member_info",
            json!({
                "group_id": group_id,
                "user_id": user_id,
                "no_cache": false
            }),
        )
        .await?;
    let data = resp.get("data").unwrap_or(&resp);
    let card = data
        .get("card")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(c) = card {
        return Some(c.to_string());
    }
    data.get("nickname")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

async fn resolve_stranger_name(runtime: &Arc<BotRuntime>, bot_id: &str, user_id: u64) -> Option<String> {
    let resp = runtime
        .call_api(
            bot_id,
            "get_stranger_info",
            json!({
                "user_id": user_id,
                "no_cache": false
            }),
        )
        .await?;
    let data = resp.get("data").unwrap_or(&resp);
    data.get("nickname")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

async fn redact_cq_at_segments(
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    group_id: u64,
    message: &str,
) -> String {
    let mut out = String::with_capacity(message.len());
    let mut rest = message;

    while let Some(start) = rest.find("[CQ:at,qq=") {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end_rel) = rest.find(']') else {
            out.push_str(rest);
            return out;
        };

        let seg = &rest[..=end_rel];
        rest = &rest[(end_rel + 1)..];

        let prefix = "[CQ:at,qq=";
        let after = &seg[prefix.len()..seg.len() - 1];
        let qq_raw = after.split(',').next().unwrap_or("").trim();

        if qq_raw.eq_ignore_ascii_case("all") {
            out.push_str("@全体成员");
            continue;
        }

        let Some(uid) = qq_raw.parse::<u64>().ok().filter(|v| *v > 0) else {
            out.push_str("@成员");
            continue;
        };

        match resolve_group_member_name(runtime, bot_id, group_id, uid).await {
            Some(name) => {
                out.push('@');
                out.push_str(&name);
            }
            None => out.push_str("@成员"),
        }
    }

    out.push_str(rest);
    out
}

async fn redact_sensitive_ids_plaintext(
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    group_id: Option<u64>,
    message: &str,
) -> String {
    fn extract_digit_tokens(s: &str, min_len: usize, max_len: usize) -> Vec<String> {
        let bytes = s.as_bytes();
        let mut i = 0usize;
        let mut out: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        while i < bytes.len() {
            if !bytes[i].is_ascii_digit() {
                i += 1;
                continue;
            }
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let end = i;
            let len = end - start;
            if len < min_len || len > max_len {
                continue;
            }
            let token = &s[start..end];
            if seen.insert(token.to_string()) {
                out.push(token.to_string());
            }
        }

        out
    }

    let ids = privacy::get_sensitive_ids();
    let mut out = message.to_string();
    let mut cache: HashMap<String, String> = HashMap::new();

    for id in ids {
        if id.is_empty() || !out.contains(&id) {
            continue;
        }

        let replacement = if let Some(v) = cache.get(&id) {
            v.clone()
        } else {
            let uid = id.parse::<u64>().ok().filter(|v| *v > 0);
            let name = match (group_id, uid) {
                (Some(gid), Some(uid)) => resolve_group_member_name(runtime, bot_id, gid, uid).await,
                (None, Some(uid)) => resolve_stranger_name(runtime, bot_id, uid).await,
                _ => None,
            };

            let replacement = name.unwrap_or_else(|| "成员".to_string());
            cache.insert(id.clone(), replacement.clone());
            replacement
        };

        out = out.replace(&id, &replacement);
    }

    // Best-effort: if the message still contains a QQ-like digit token, resolve it to nickname.
    // This catches cases like "踢出 123456789" even when the ID isn't in the current event context.
    let mut attempts = 0usize;
    for token in extract_digit_tokens(&out, 5, 12) {
        if attempts >= 8 {
            break;
        }
        if token.is_empty() || !out.contains(&token) {
            continue;
        }
        if cache.contains_key(&token) {
            if let Some(repl) = cache.get(&token).cloned() {
                out = out.replace(&token, &repl);
            }
            continue;
        }

        let uid = token.parse::<u64>().ok().filter(|v| *v > 0);
        let Some(uid) = uid else { continue };
        attempts += 1;

        let name = if let Some(gid) = group_id {
            if let Some(name) = resolve_group_member_name(runtime, bot_id, gid, uid).await {
                Some(name)
            } else {
                resolve_stranger_name(runtime, bot_id, uid).await
            }
        } else {
            resolve_stranger_name(runtime, bot_id, uid).await
        };
        let Some(name) = name else {
            continue;
        };

        cache.insert(token.clone(), name.clone());
        out = out.replace(&token, &name);
    }

    out
}

async fn sanitize_outgoing_text(
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    group_id: Option<u64>,
    message: &str,
) -> String {
    let msg = match group_id {
        Some(gid) => redact_cq_at_segments(runtime, bot_id, gid, message).await,
        None => message.to_string(),
    };
    redact_sensitive_ids_plaintext(runtime, bot_id, group_id, &msg).await
}

async fn sanitize_onebot_message_value(
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    group_id: Option<u64>,
    value: &Value,
) -> Value {
    match value {
        Value::String(s) => Value::String(sanitize_outgoing_text(runtime, bot_id, group_id, s).await),
        Value::Array(arr) => {
            let mut out: Vec<Value> = Vec::with_capacity(arr.len());
            for seg in arr {
                let ty = seg.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if ty == "at" {
                    let qq_raw: Option<String> = seg
                        .get("data")
                        .and_then(|d| d.get("qq"))
                        .and_then(|v| match v {
                            Value::String(s) => Some(s.trim().to_string()),
                            Value::Number(n) => n.as_u64().map(|u| u.to_string()),
                            _ => None,
                        });

                    let qq_raw = qq_raw.unwrap_or_default();
                    let text = if qq_raw.eq_ignore_ascii_case("all") {
                        "@全体成员".to_string()
                    } else if let (Some(gid), Ok(uid)) = (group_id, qq_raw.parse::<u64>()) {
                        match resolve_group_member_name(runtime, bot_id, gid, uid).await {
                            Some(name) => format!("@{}", name),
                            None => "@成员".to_string(),
                        }
                    } else {
                        "@成员".to_string()
                    };
                    out.push(json!({"type":"text","data":{"text": text}}));
                    continue;
                }

                if ty == "text" {
                    if let Some(text) = seg.get("data").and_then(|d| d.get("text")).and_then(|v| v.as_str()) {
                        let text = sanitize_outgoing_text(runtime, bot_id, group_id, text).await;
                        out.push(json!({"type":"text","data":{"text": text}}));
                        continue;
                    }
                }

                out.push(seg.clone());
            }
            Value::Array(out)
        }
        _ => value.clone(),
    }
}

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
        let message = sanitize_outgoing_text(runtime, bot_id, Some(gid), message).await;

        match runtime.get_group_send_status(bot_id, gid).await {
            GroupSendStatus::Muted => {
                warn!("[{}] 群 {} 内机器人被禁言，跳过发送群消息", bot_id, gid);
                return;
            }
            GroupSendStatus::Unknown | GroupSendStatus::Allowed => {}
        }

        // 去重检查：相同消息在5秒内不重复发送（仅在允许发送时才记录）
        let hash = compute_message_hash(bot_id, gid, &message);
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
        let message = sanitize_outgoing_text(runtime, bot_id, None, message).await;

        // 去重检查：相同消息在5秒内不重复发送（仅在允许发送时才记录）
        let hash = compute_message_hash(bot_id, user_id, &message);
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
    let mut params = params;

    let group_id = extract_group_id_for_send_action(action, &params);
    if let Some(gid) = group_id {
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

    // Privacy: sanitize outgoing messages (CQ @ -> nickname; redact sensitive numeric IDs in plain text).
    match action {
        "send_group_msg" | "send_private_msg" | "send_msg" | "send_forward_msg" => {
            if let Some(v) = params.get("message").cloned() {
                let sanitized = sanitize_onebot_message_value(runtime, bot_id, group_id, &v).await;
                if sanitized != v {
                    params["message"] = sanitized;
                }
            }
        }
        "send_group_forward_msg" | "send_private_forward_msg" => {
            if let Some(Value::Array(nodes)) = params.get("messages").cloned() {
                let mut out_nodes: Vec<Value> = Vec::with_capacity(nodes.len());
                for node in nodes {
                    let mut node = node;
                    let data = node.get_mut("data");
                    if let Some(Value::Object(_)) = data {
                        if let Some(name) = node
                            .get("data")
                            .and_then(|d| d.get("name"))
                            .and_then(|v| v.as_str())
                        {
                            let name = sanitize_outgoing_text(runtime, bot_id, group_id, name).await;
                            node["data"]["name"] = Value::String(name);
                        }
                        if let Some(content) = node.get("data").and_then(|d| d.get("content")).cloned() {
                            let content = sanitize_onebot_message_value(runtime, bot_id, group_id, &content).await;
                            node["data"]["content"] = content;
                        }
                    }
                    out_nodes.push(node);
                }
                params["messages"] = Value::Array(out_nodes);
            }
        }
        _ => {}
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
