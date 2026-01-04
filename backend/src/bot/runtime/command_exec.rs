use crate::command::{Command, CommandAction};
use crate::models::SharedState;
use serde_json::json;
use std::sync::Arc;
use tracing::warn;

use super::api::send_reply;
use super::connection::BotRuntime;
use super::help_image::generate_help_image;
use super::message::is_admin;

mod llm_abuse;
mod llm_forward;
mod plugin_outputs;

pub struct CommandExecInput<'a> {
    pub user_id: u64,
    pub group_id: Option<u64>,
    pub command_used: &'a str,
    pub args: &'a [&'a str],
    pub raw_message: Option<&'a str>,
    pub message: Option<&'a serde_json::Value>,
    pub reply_message: Option<&'a serde_json::Value>,
}

pub async fn execute_command(
    state: &SharedState,
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    command: &Command,
    input: CommandExecInput<'_>,
) {
    let user_id = input.user_id;
    let group_id = input.group_id;
    let args = input.args;
    let raw_message = input.raw_message;
    let message = input.message;
    let reply_message = input.reply_message;

    state.message_stats.inc_call();

    match &command.action {
        CommandAction::Help => {
            let help_cmd = state.commands.get("help");
            let mode = help_cmd
                .as_ref()
                .and_then(|c| c.config.get("mode"))
                .and_then(|v| v.as_str())
                .unwrap_or("text");

            if mode == "image" {
                match generate_help_image(state, bot_id).await {
                    Ok(img_base64) => {
                        let img_msg = format!("[CQ:image,file=base64://{}]", img_base64);
                        send_reply(runtime, bot_id, user_id, group_id, &img_msg).await;
                    }
                    Err(e) => {
                        warn!("[{}] 帮助图片生成失败: {}", bot_id, e);
                        send_reply(
                            runtime,
                            bot_id,
                            user_id,
                            group_id,
                            "帮助图片生成失败：wkhtmltoimage 不可用或渲染失败",
                        )
                        .await;
                    }
                };
            } else {
                let help_text = generate_help_text(state, bot_id);
                send_reply(runtime, bot_id, user_id, group_id, &help_text).await;
            }
        }
        CommandAction::Plugin(plugin_id) => {
            let is_admin = is_admin(state, bot_id, user_id);
            let is_super_admin = super::message::is_super_admin(state, bot_id, user_id);
            let command_used = input.command_used;
            let is_alias = command_used != command.name;
            let ctx = json!({
                "command": command.name,
                "command_used": command_used,
                "command_is_alias": is_alias,
                "user_id": user_id,
                "group_id": group_id,
                "args": args,
                "raw_message": raw_message,
                "message": message,
                "reply_message": reply_message,
                "is_admin": is_admin,
                "is_super_admin": is_super_admin,
            });

            match state.plugin_manager.on_command(plugin_id, ctx).await {
                Ok(outputs) => {
                    plugin_outputs::process_plugin_outputs(state, runtime, bot_id, &outputs).await
                }
                Err(e) => {
                    warn!("[{}] 插件 {} onCommand 失败: {}", bot_id, plugin_id, e);
                    send_reply(
                        runtime,
                        bot_id,
                        user_id,
                        group_id,
                        "插件执行失败：请查看后台日志",
                    )
                    .await;
                }
            }
        }
        CommandAction::Custom(action) => {
            send_reply(
                runtime,
                bot_id,
                user_id,
                group_id,
                &format!("自定义指令: {}", action),
            )
            .await;
        }
    }
}

/// 处理带有来源插件 ID 的输出列表，支持 LLM 回调
pub(super) async fn process_plugin_outputs_with_source(
    state: &SharedState,
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    outputs: &[crate::plugin::PluginOutputWithSource],
) {
    plugin_outputs::process_plugin_outputs_with_source(state, runtime, bot_id, outputs).await
}

fn generate_help_text(state: &SharedState, bot_id: &str) -> String {
    let prefix = super::message::get_command_prefix(state, bot_id);
    let mut text = String::new();

    fn shorten(input: &str, max_chars: usize) -> String {
        if max_chars == 0 {
            return String::new();
        }
        let count = input.chars().count();
        if count <= max_chars {
            return input.to_string();
        }
        let mut out = String::new();
        for (i, ch) in input.chars().enumerate() {
            if i + 1 >= max_chars {
                break;
            }
            out.push(ch);
        }
        out.push('…');
        out
    }

    fn owner_label(state: &SharedState, cmd: &Command) -> String {
        if cmd.is_builtin {
            return "内置".to_string();
        }
        match &cmd.action {
            CommandAction::Plugin(pid) => state
                .plugins
                .get(pid)
                .map(|p| p.manifest.name)
                .unwrap_or_else(|| pid.to_string()),
            CommandAction::Custom(_) => "自定义".to_string(),
            CommandAction::Help => "内置".to_string(),
        }
    }

    // De-duplicate and keep deterministic priority: builtin > plugin > custom.
    let mut unique: std::collections::BTreeMap<String, Command> = std::collections::BTreeMap::new();
    for cmd in state.commands.list() {
        let key = cmd.name.trim().to_ascii_lowercase();
        let p = if cmd.is_builtin {
            3u8
        } else {
            match cmd.action {
                CommandAction::Plugin(_) => 2,
                CommandAction::Custom(_) => 1,
                CommandAction::Help => 3,
            }
        };
        match unique.get(&key) {
            None => {
                unique.insert(key, cmd);
            }
            Some(existing) => {
                let ep = if existing.is_builtin {
                    3u8
                } else {
                    match existing.action {
                        CommandAction::Plugin(_) => 2,
                        CommandAction::Custom(_) => 1,
                        CommandAction::Help => 3,
                    }
                };
                if p > ep || (p == ep && cmd.id < existing.id) {
                    unique.insert(key, cmd);
                }
            }
        }
    }

    let mut cmds: Vec<String> = Vec::new();
    for (_key, cmd) in unique {
        let owner = shorten(&owner_label(state, &cmd), 10);
        cmds.push(format!("{}{} [{}]", prefix, cmd.name, owner));
    }

    text.push_str(&format!("指令菜单（{} 个）\n\n", cmds.len()));

    let mut width = cmds.iter().map(|s| s.chars().count()).max().unwrap_or(0);
    width = width.clamp(8, 24);

    for row in cmds.chunks(2) {
        match row {
            [a, b] => {
                text.push_str(&format!("{:<width$}  {}\n", a, b, width = width));
            }
            [a] => {
                text.push_str(a);
                text.push('\n');
            }
            _ => {}
        }
    }

    let mut features: Vec<(String, String)> = state
        .plugins
        .list_enabled()
        .into_iter()
        .filter(|p| p.manifest.commands.is_empty())
        .map(|p| {
            let name = p.manifest.name.trim().to_string();
            let desc_raw = p.manifest.description.trim().to_string();
            let desc = if desc_raw.is_empty() {
                "（缺少简介：请在 manifest.json 填写 description）".to_string()
            } else {
                desc_raw
            };
            (name, desc)
        })
        .collect();
    features.sort_by(|a, b| a.0.cmp(&b.0));

    if !features.is_empty() {
        text.push_str("\n\n插件功能（无指令）\n");
        for (name, desc) in features {
            text.push_str(&format!("- {}：{}\n", name, desc));
        }
    }

    text.push_str("\n\n提示：在 WebUI 的「指令管理」可查看详细说明与别名；在「插件中心」可查看插件简介与配置。");
    text
}
