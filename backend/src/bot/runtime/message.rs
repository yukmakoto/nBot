use crate::command::{Command, CommandAction};
use crate::models::SharedState;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;
use tracing::info;

use super::command_exec::{execute_command, process_plugin_outputs_with_source, CommandExecInput};
use super::connection::{BotRuntime, GroupSendStatus};
use super::privacy;

mod reply;

fn parse_u64_field(v: Option<&Value>) -> Option<u64> {
    match v? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse::<u64>().ok(),
        _ => None,
    }
}

fn message_at_self(event: &serde_json::Value, self_id: u64) -> bool {
    let Some(segments) = event.get("message").and_then(|m| m.as_array()) else {
        return false;
    };

    for seg in segments {
        if seg.get("type").and_then(|t| t.as_str()) != Some("at") {
            continue;
        }
        let qq = parse_u64_field(seg.get("data").and_then(|d| d.get("qq")));
        if qq == Some(self_id) {
            return true;
        }
    }

    false
}

fn parse_reply_id_from_raw(raw_message: &str) -> Option<u64> {
    let tag = "[CQ:reply";
    let start = raw_message.find(tag)?;
    let after = &raw_message[start + tag.len()..];
    let id_key = "id=";
    let id_pos = after.find(id_key)?;
    let after_id = &after[id_pos + id_key.len()..];
    let digits: String = after_id
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

fn collect_cq_at_ids_from_raw(raw_message: &str, out: &mut HashSet<String>) {
    let mut rest = raw_message;
    while let Some(start) = rest.find("[CQ:at,qq=") {
        rest = &rest[start + "[CQ:at,qq=".len()..];
        let end = match rest.find(']') {
            Some(i) => i,
            None => break,
        };
        let seg = &rest[..end];
        rest = &rest[end + 1..];

        let qq_raw = seg.split(',').next().unwrap_or("").trim();
        if qq_raw.eq_ignore_ascii_case("all") || qq_raw.is_empty() {
            continue;
        }
        if qq_raw.chars().all(|c| c.is_ascii_digit()) {
            out.insert(qq_raw.to_string());
        }
    }
}

fn decode_basic_html_entities(s: &str) -> String {
    // Minimal decoding for URLs and CQ segment fields (NapCat sometimes returns &amp; in url).
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&#91;", "[")
        .replace("&#93;", "]")
}

fn parse_cq_field(raw: &str, key: &str) -> Option<String> {
    // Parse `[CQ:...,key=value,...]` field from raw_message.
    let needle = format!("{key}=");
    let idx = raw.find(&needle)?;
    let after = &raw[idx + needle.len()..];
    let end = after
        .find(',')
        .or_else(|| after.find(']'))
        .unwrap_or(after.len());
    let val = after[..end].trim();
    if val.is_empty() {
        None
    } else {
        Some(decode_basic_html_entities(val))
    }
}

fn extract_command_line(
    event: &serde_json::Value,
    raw_message: &str,
    prefix: &str,
) -> Option<String> {
    let raw_trim = raw_message.trim_start();
    if raw_trim.starts_with(prefix) {
        return Some(raw_trim.to_string());
    }

    if let Some(segments) = event.get("message").and_then(|m| m.as_array()) {
        for seg in segments {
            if seg.get("type").and_then(|t| t.as_str()) != Some("text") {
                continue;
            }
            if let Some(text) = seg
                .get("data")
                .and_then(|d| d.get("text"))
                .and_then(|v| v.as_str())
            {
                let trimmed = text.trim_start();
                if trimmed.starts_with(prefix) {
                    return Some(trimmed.to_string());
                }
            }
        }
    }

    let mut s = raw_trim;
    while s.starts_with("[CQ:") {
        let end = s.find(']')?;
        s = s[end + 1..].trim_start();
    }
    if s.starts_with(prefix) {
        return Some(s.to_string());
    }

    None
}

pub async fn handle_event(
    state: &SharedState,
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    event: Value,
) {
    // API 响应已在 connection.rs 中直接处理，这里只处理其他事件
    let post_type = event
        .get("post_type")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match post_type {
        "message" => handle_message(state, runtime, bot_id, event).await,
        "meta_event" => handle_meta_event(state, runtime, bot_id, event).await,
        "notice" => handle_notice(state, runtime, bot_id, event).await,
        "request" => info!(
            "[{}] 请求: {}",
            bot_id,
            event
                .get("request_type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
        ),
        _ => {}
    }
}

async fn handle_meta_event(
    state: &SharedState,
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    event: Value,
) {
    let meta_event_type = event["meta_event_type"].as_str().unwrap_or("unknown");
    if meta_event_type != "heartbeat" {
        return;
    }

    let self_id = runtime.get_self_id(bot_id).await;
    let self_id_str = self_id.map(|sid| sid.to_string());

    let meta_ctx = json!({
        "meta_event_type": meta_event_type,
        "self_id": self_id,
        "self_id_str": self_id_str,
        "time": event.get("time").cloned().unwrap_or(Value::Null),
        "status": event.get("status").cloned().unwrap_or(Value::Null),
        "interval": event.get("interval").cloned().unwrap_or(Value::Null),
    });

    // Call plugins (best-effort), e.g. heartbeat-driven tasks.
    let result = state.plugin_manager.on_meta_event(meta_ctx).await;
    process_plugin_outputs_with_source(state, runtime, bot_id, &result.outputs).await;
}

async fn handle_message(
    state: &SharedState,
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    event: Value,
) {
    let message_type = event["message_type"].as_str().unwrap_or("unknown");
    let user_id = parse_u64_field(event.get("user_id")).unwrap_or(0);
    let group_id = parse_u64_field(event.get("group_id"));
    let raw_message = event["raw_message"].as_str().unwrap_or("").to_string();
    let user_id_raw = event.get("user_id").cloned().unwrap_or(Value::Null);
    let group_id_raw = event.get("group_id").cloned().unwrap_or(Value::Null);
    let user_id_str = user_id.to_string();
    let group_id_str = group_id.map(|g| g.to_string());

    // 忽略机器人自身消息，避免插件/指令对“自己发出去的消息”重复处理导致循环或滥用。
    let self_id = runtime.get_self_id(bot_id).await;
    if let Some(sid) = self_id {
        if user_id == sid {
            info!("[{}] 忽略机器人自身消息", bot_id);
            return;
        }
    }

    let at_bot = self_id.map(|sid| message_at_self(&event, sid)).unwrap_or(false);
    let self_id_str = self_id.map(|sid| sid.to_string());

    // 统计消息
    state.message_stats.check_reset().await;
    state.message_stats.inc_message();

    let mut sensitive_ids: HashSet<String> = HashSet::new();
    if user_id > 0 {
        sensitive_ids.insert(user_id.to_string());
    }
    if let Some(sid) = self_id {
        sensitive_ids.insert(sid.to_string());
    }
    collect_cq_at_ids_from_raw(&raw_message, &mut sensitive_ids);
    if let Some(segments) = event.get("message").and_then(|m| m.as_array()) {
        for seg in segments {
            if seg.get("type").and_then(|t| t.as_str()) != Some("at") {
                continue;
            }
            let qq = seg
                .get("data")
                .and_then(|d| d.get("qq"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("all"))
                .filter(|s| s.chars().all(|c| c.is_ascii_digit()));
            if let Some(qq) = qq {
                sensitive_ids.insert(qq.to_string());
            }
        }
    }

    privacy::with_sensitive_ids(sensitive_ids, async {
        info!(
            "[{}] 收到消息 ({}) from {}: {}",
            bot_id,
            message_type,
            user_id,
            raw_message.as_str()
        );

        let is_admin = is_admin(state, bot_id, user_id);
        let is_super_admin = is_super_admin(state, bot_id, user_id);

        // If the message is replying to another message, fetch the replied content so plugins can use it.
        let reply_message = reply::get_reply_message_content(runtime, bot_id, group_id, &event).await;

        // 调用插件 preMessage 钩子（包括白名单过滤等）
        let pre_msg_ctx = json!({
            // Keep original type for plugin (number/string), and also provide string forms for safety.
            "user_id": user_id_raw.clone(),
            "user_id_str": user_id_str.clone(),
            "group_id": group_id_raw.clone(),
            "group_id_str": group_id_str.clone(),
            "self_id": self_id,
            "self_id_str": self_id_str,
            "at_bot": at_bot,
            "message_type": message_type,
            "raw_message": raw_message.as_str(),
            "message_id": event.get("message_id").cloned().unwrap_or(Value::Null),
            "message": event.get("message").cloned().unwrap_or(Value::Null),
            "reply_message": reply_message.as_ref(),
            "is_admin": is_admin,
            "is_super_admin": is_super_admin,
        });
        let pre_msg_result = state.plugin_manager.pre_message(pre_msg_ctx).await;

        // 处理插件输出（支持 LLM 回调）
        process_plugin_outputs_with_source(state, runtime, bot_id, &pre_msg_result.outputs).await;

        if !pre_msg_result.allow && !is_super_admin {
            info!("[{}] 消息被插件过滤", bot_id);
            return;
        }

        // 指令模块未启用 - 不处理指令
        if !crate::module::is_module_enabled(state, bot_id, "command") {
            return;
        }

        // 获取指令前缀
        let prefix = get_command_prefix(state, bot_id);
        let command_line = extract_command_line(&event, &raw_message, &prefix);

        // 非指令消息 - 直接忽略
        let command_line = match command_line {
            Some(line) => line,
            None => return,
        };
        if !command_line.starts_with(&prefix) {
            return;
        }

        let cmd_text = &command_line[prefix.len()..];
        let parts: Vec<&str> = cmd_text.split_whitespace().collect();
        if parts.is_empty() {
            return;
        }

        let cmd_name = parts[0];
        let args: Vec<&str> = parts[1..].to_vec();

        // 群聊内如果机器人无法发言，则不执行指令（避免“无响应/浪费资源/报错”）
        if let Some(gid) = group_id {
            if matches!(
                runtime.get_group_send_status(bot_id, gid).await,
                GroupSendStatus::Muted
            ) {
                info!(
                    "[{}] 群 {} 内机器人被禁言，跳过执行指令: {}",
                    bot_id, gid, cmd_name
                );
                return;
            }
        }

        if let Some(command) = find_command(state, cmd_name) {
            // 检查是否有回复消息，如果有则获取被回复消息的内容
            let reply_message =
                reply::get_reply_message_content(runtime, bot_id, group_id, &event).await;

            // 调用插件 preCommand 钩子
            let ctx = json!({
                "user_id": user_id_raw,
                "user_id_str": user_id_str,
                "group_id": group_id_raw,
                "group_id_str": group_id_str,
                "command": command.name,
                "command_used": cmd_name,
                "command_is_alias": cmd_name != command.name,
                "args": args,
                "raw_message": raw_message.as_str(),
                "message": event.get("message"),
                "reply_message": reply_message.as_ref(),
                "is_admin": is_admin,
                "is_super_admin": is_super_admin,
            });
            let pre_cmd_result = state.plugin_manager.pre_command(ctx).await;

            // 处理插件输出（支持 LLM 回调）
            process_plugin_outputs_with_source(state, runtime, bot_id, &pre_cmd_result.outputs)
                .await;

            if !pre_cmd_result.allow && !is_super_admin {
                info!("[{}] 指令 {} 被插件阻止", bot_id, command.name);
                return;
            }
            info!("[{}] 执行指令: {}", bot_id, command.name);
            execute_command(
                state,
                runtime,
                bot_id,
                &command,
                CommandExecInput {
                    user_id,
                    group_id,
                    command_used: cmd_name,
                    args: &args,
                    raw_message: Some(raw_message.as_str()),
                    message: event.get("message"),
                    reply_message: reply_message.as_ref(),
                },
            )
            .await;
        }
    })
    .await;
}

/// 检查是否为管理员
pub fn is_admin(state: &SharedState, bot_id: &str, user_id: u64) -> bool {
    let module = match crate::module::get_effective_module(state, bot_id, "admin") {
        Some(m) if m.enabled => m,
        _ => return false,
    };

    let user_str = user_id.to_string();
    let admins: Vec<String> = module.config["admins"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let super_admins: Vec<String> = module.config["super_admins"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    admins.contains(&user_str) || super_admins.contains(&user_str)
}

pub fn is_super_admin(state: &SharedState, bot_id: &str, user_id: u64) -> bool {
    let module = match crate::module::get_effective_module(state, bot_id, "admin") {
        Some(m) => m,
        None => return false,
    };

    let user_str = user_id.to_string();
    module
        .config
        .get("super_admins")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str())
        .any(|id| id == user_str.as_str())
}

/// 获取指令前缀
pub fn get_command_prefix(state: &SharedState, bot_id: &str) -> String {
    if let Some(m) = crate::module::get_effective_module(state, bot_id, "command") {
        if let Some(prefix) = m.config["prefix"].as_str() {
            return prefix.to_string();
        }
    }
    "/".to_string()
}

pub fn find_command(state: &SharedState, name: &str) -> Option<Command> {
    let name_owned = name.to_string();
    let mut best: Option<(u8, String, Command)> = None;

    for cmd in state.commands.list() {
        let exact = cmd.name == name;
        let alias = !exact && cmd.aliases.contains(&name_owned);
        if !(exact || alias) {
            continue;
        }

        // Priority:
        // - builtin > plugin > custom
        // - within same kind: exact > alias
        //
        // This makes command resolution deterministic and prevents custom commands from shadowing
        // shipped plugin commands with the same name/alias.
        let kind: u8 = if cmd.is_builtin {
            3
        } else {
            match cmd.action {
                CommandAction::Plugin(_) => 2,
                CommandAction::Custom(_) => 1,
                CommandAction::Help => 3,
            }
        };
        let m: u8 = if exact { 1 } else { 0 };
        let score = kind * 2 + m;

        let id = cmd.id.clone();
        match &best {
            None => best = Some((score, id, cmd)),
            Some((best_score, best_id, _)) => {
                if score > *best_score || (score == *best_score && id < *best_id) {
                    best = Some((score, id, cmd));
                }
            }
        }
    }

    best.map(|(_, _, cmd)| cmd)
}

/// 处理 notice 事件（通知类事件，如灰条消息、成员变动等）
async fn handle_notice(
    state: &SharedState,
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    event: Value,
) {
    let notice_type = event["notice_type"].as_str().unwrap_or("unknown");
    let sub_type = event["sub_type"].as_str().unwrap_or("");

    info!(
        "[{}] 通知: {} {}",
        bot_id, notice_type, sub_type
    );

    // 构建通用的 notice 上下文
    let user_id = parse_u64_field(event.get("user_id")).unwrap_or(0);
    let group_id = parse_u64_field(event.get("group_id"));
    let operator_id = parse_u64_field(event.get("operator_id")).unwrap_or(0);
    let self_id = runtime.get_self_id(bot_id).await;
    let self_id_str = self_id.map(|sid| sid.to_string());

    let (bot_is_admin, bot_role) = if let (Some(gid), Some(sid)) = (group_id, self_id) {
        // Only check for relevant notice types to avoid extra API calls.
        let need_check = matches!(
            notice_type,
            "group_increase" | "group_decrease" | "group_admin" | "group_ban"
        );
        if need_check {
            match runtime
                .call_api(
                    bot_id,
                    "get_group_member_info",
                    json!({ "group_id": gid, "user_id": sid, "no_cache": true }),
                )
                .await
            {
                Some(resp) if resp.get("status").and_then(|s| s.as_str()) == Some("ok") => {
                    let data = resp.get("data").unwrap_or(&Value::Null);
                    let role_raw = data.get("role").cloned().unwrap_or(Value::Null);
                    let role_s = role_raw
                        .as_str()
                        .unwrap_or("")
                        .trim()
                        .to_ascii_lowercase();

                    let admin_flag = data.get("is_admin").and_then(|v| v.as_bool()) == Some(true)
                        || data.get("is_owner").and_then(|v| v.as_bool()) == Some(true)
                        || data.get("admin").and_then(|v| v.as_bool()) == Some(true);

                    let admin_by_role = matches!(role_s.as_str(), "admin" | "owner" | "administrator")
                        || role_s.contains("admin")
                        || role_s.contains("owner");

                    let admin_by_number = role_raw.as_i64().is_some_and(|n| n == 1 || n == 2);

                    (
                        admin_flag || admin_by_role || admin_by_number,
                        if role_s.is_empty() { None } else { Some(role_s) },
                    )
                }
                _ => (false, None),
            }
        } else {
            (false, None)
        }
    } else {
        (false, None)
    };

    let mut sensitive_ids: HashSet<String> = HashSet::new();
    if user_id > 0 {
        sensitive_ids.insert(user_id.to_string());
    }
    if operator_id > 0 {
        sensitive_ids.insert(operator_id.to_string());
    }
    if let Some(sid) = self_id {
        sensitive_ids.insert(sid.to_string());
    }

    let notice_ctx = match notice_type {
        // 灰条消息事件
        "notify" if sub_type == "gray_tip" => {
            let message_id = event.get("message_id").cloned().unwrap_or(Value::Null);
            let busi_id = event["busi_id"].as_str().unwrap_or("");
            let content = event["content"].as_str().unwrap_or("");

            json!({
                "notice_type": notice_type,
                "sub_type": sub_type,
                "user_id": user_id,
                "group_id": group_id,
                "self_id": self_id,
                "self_id_str": self_id_str,
                "message_id": message_id,
                "busi_id": busi_id,
                "content": content,
                "raw_info": event.get("raw_info").cloned().unwrap_or(Value::Null),
            })
        }
        // 群成员增加事件
        "group_increase" => {
            json!({
                "notice_type": notice_type,
                "sub_type": sub_type, // "approve" (管理员同意) 或 "invite" (被邀请)
                "user_id": user_id,   // 新成员 QQ
                "group_id": group_id,
                "self_id": self_id,
                "self_id_str": self_id_str,
                "operator_id": operator_id, // 操作者 QQ（同意入群的管理员或邀请者）
                "bot_is_admin": bot_is_admin,
                "bot_role": bot_role,
            })
        }
        // 群成员减少事件
        "group_decrease" => {
            json!({
                "notice_type": notice_type,
                "sub_type": sub_type, // "leave" (主动退群), "kick" (被踢), "kick_me" (机器人被踢)
                "user_id": user_id,   // 离开的成员 QQ
                "group_id": group_id,
                "self_id": self_id,
                "self_id_str": self_id_str,
                "operator_id": operator_id, // 操作者 QQ（踢人的管理员）
                "bot_is_admin": bot_is_admin,
                "bot_role": bot_role,
            })
        }
        // 群管理员变动
        "group_admin" => {
            json!({
                "notice_type": notice_type,
                "sub_type": sub_type, // "set" (设置管理员) 或 "unset" (取消管理员)
                "user_id": user_id,   // 被操作的成员 QQ
                "group_id": group_id,
                "self_id": self_id,
                "self_id_str": self_id_str,
                "bot_is_admin": bot_is_admin,
                "bot_role": bot_role,
            })
        }
        // 群禁言
        "group_ban" => {
            let duration = event["duration"].as_u64().unwrap_or(0);
            json!({
                "notice_type": notice_type,
                "sub_type": sub_type, // "ban" (禁言) 或 "lift_ban" (解除禁言)
                "user_id": user_id,   // 被禁言的成员 QQ
                "group_id": group_id,
                "self_id": self_id,
                "self_id_str": self_id_str,
                "operator_id": operator_id,
                "duration": duration, // 禁言时长（秒），0 表示解除禁言
                "bot_is_admin": bot_is_admin,
                "bot_role": bot_role,
            })
        }
        // 其他通知类型，传递原始事件
        _ => {
            json!({
                "notice_type": notice_type,
                "sub_type": sub_type,
                "user_id": user_id,
                "group_id": group_id,
                "self_id": self_id,
                "self_id_str": self_id_str,
                "operator_id": operator_id,
                "raw_event": event,
                "bot_is_admin": bot_is_admin,
                "bot_role": bot_role,
            })
        }
    };

    privacy::with_sensitive_ids(sensitive_ids, async {
        // 调用插件 onNotice 钩子
        let notice_result = state.plugin_manager.on_notice(notice_ctx).await;

        // 处理插件输出（支持 LLM 回调）
        process_plugin_outputs_with_source(state, runtime, bot_id, &notice_result.outputs).await;
    })
    .await;
}
