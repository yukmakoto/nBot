use crate::bot::runtime::api::{send_api, send_reply};
use crate::bot::runtime::BotRuntime;
use crate::models::SharedState;
use crate::render_image::render_markdown_image;
use serde_json::json;
use std::sync::Arc;
use tracing::error;

pub(in super::super::super) struct SendForwardImageInput<'a> {
    pub(in super::super::super) user_id: u64,
    pub(in super::super::super) group_id: u64,
    pub(in super::super::super) title: &'a str,
    pub(in super::super::super) markdown: &'a str,
}

pub(in super::super::super) async fn send_llm_markdown_as_forward_image(
    state: &SharedState,
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    input: SendForwardImageInput<'_>,
) {
    let user_id = input.user_id;
    let group_id = input.group_id;
    let title = input.title;
    let markdown = super::super::super::redact::redact_qq_ids(input.markdown);

    let group_id_opt = (group_id != 0).then_some(group_id);
    let now = chrono::Local::now();

    let img_base64 = match render_markdown_image(title, "分析报告", &markdown, 520).await {
        Ok(img) => img,
        Err(e) => {
            error!("[{}] render_markdown_image failed: {}", bot_id, e);
            send_reply(
                runtime,
                bot_id,
                user_id,
                group_id_opt,
                "分析失败：结果渲染为图片失败（wkhtmltoimage 不可用或渲染报错）",
            )
            .await;
            return;
        }
    };

    let bot_name = state
        .bots
        .get(bot_id)
        .map(|b| b.value().name.clone())
        .unwrap_or_else(|| "nBot".to_string());

    let bot_qq = state
        .bots
        .get(bot_id)
        .and_then(|b| {
            b.value()
                .qq_id
                .as_deref()
                .and_then(|s| s.parse::<u64>().ok())
        })
        .or_else(|| {
            bot_id
                .strip_prefix("qq_")
                .and_then(|s| s.parse::<u64>().ok())
        })
        .or_else(|| bot_id.parse::<u64>().ok())
        .unwrap_or(10000);

    let mut nodes: Vec<serde_json::Value> = vec![
        json!({
            "type": "node",
            "data": {
                "name": bot_name,
                "uin": bot_qq.to_string(),
                "content": format!("{}\nTime: {}", title, now.format("%Y-%m-%d %H:%M:%S"))
            }
        }),
        json!({
            "type": "node",
            "data": {
                "name": bot_name,
                "uin": bot_qq.to_string(),
                "content": format!("[CQ:image,file=base64://{}]", img_base64)
            }
        }),
    ];

    for content in super::super::super::output_extract::build_plain_supplement_nodes(&markdown) {
        nodes.push(json!({
            "type": "node",
            "data": {
                "name": bot_name,
                "uin": bot_qq.to_string(),
                "content": content
            }
        }));
    }

    if let Some(gid) = group_id_opt {
        send_api(
            runtime,
            bot_id,
            "send_group_forward_msg",
            json!({ "group_id": gid, "messages": nodes }),
        )
        .await;
    } else {
        send_api(
            runtime,
            bot_id,
            "send_private_forward_msg",
            json!({ "user_id": user_id, "messages": nodes }),
        )
        .await;
    }
}
