use deno_core::{op2, OpState};
use tracing::{error, info};

use super::{PluginOpState, PluginOutput};

// Op: Send message to QQ group (legacy, use op_send_reply instead)
#[op2(fast)]
pub(in super::super) fn op_send_message(
    state: &mut OpState,
    #[bigint] group_id: i64,
    #[string] content: &str,
) {
    state
        .borrow_mut::<PluginOpState>()
        .outputs
        .push(PluginOutput::SendReply {
            user_id: 0,
            group_id: if group_id > 0 {
                Some(group_id as u64)
            } else {
                None
            },
            content: content.to_string(),
        });
}

// Op: 发送回复消息
#[op2(fast)]
pub(in super::super) fn op_send_reply(
    state: &mut OpState,
    #[bigint] user_id: i64,
    #[bigint] group_id: i64,
    #[string] content: &str,
) {
    state
        .borrow_mut::<PluginOpState>()
        .outputs
        .push(PluginOutput::SendReply {
            user_id: user_id as u64,
            group_id: if group_id > 0 {
                Some(group_id as u64)
            } else {
                None
            },
            content: content.to_string(),
        });
}

// Op: 调用 QQ API
#[op2(fast)]
pub(in super::super) fn op_call_api(
    state: &mut OpState,
    #[string] action: &str,
    #[string] params_json: &str,
) {
    let params: serde_json::Value = match serde_json::from_str(params_json) {
        Ok(v) => v,
        Err(e) => {
            super::log_json_parse_error(&*state, "callApi(params)", &e);
            return;
        }
    };
    state
        .borrow_mut::<PluginOpState>()
        .outputs
        .push(PluginOutput::CallApi {
            action: action.to_string(),
            params,
        });
}

// Op: Log from plugin
#[op2(fast)]
pub(in super::super) fn op_log(
    state: &mut OpState,
    #[string] level: &str,
    #[string] message: &str,
) {
    let plugin_id = state.borrow::<PluginOpState>().plugin_id.clone();
    match level {
        "info" => info!("[插件:{}] {}", plugin_id, message),
        "warn" => tracing::warn!("[插件:{}] {}", plugin_id, message),
        "error" => error!("[插件:{}] {}", plugin_id, message),
        _ => info!("[插件:{}] {}", plugin_id, message),
    }
}

// Op: 设置钩子返回值
#[op2(fast)]
pub(in super::super) fn op_set_hook_result(state: &mut OpState, result: bool) {
    state.borrow_mut::<PluginOpState>().hook_result = Some(result);
}

// Op: 获取当前时间戳（毫秒）
#[op2(fast)]
pub(in super::super) fn op_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

// Op: 获取插件配置
#[op2]
#[string]
pub(in super::super) fn op_get_config(state: &mut OpState) -> String {
    serde_json::to_string(&state.borrow::<PluginOpState>().config)
        .unwrap_or_else(|_| "{}".to_string())
}

// Op: 设置插件配置（会写回 manifest.json 并热更新到运行时）
#[op2(fast)]
pub(in super::super) fn op_set_config(state: &mut OpState, #[string] config_json: &str) -> bool {
    let config: serde_json::Value = match serde_json::from_str(config_json) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let plugin_id = state.borrow::<PluginOpState>().plugin_id.clone();

    {
        let st = state.borrow_mut::<PluginOpState>();
        st.config = config.clone();
        st.outputs
            .push(PluginOutput::UpdateConfig { plugin_id, config });
    }

    true
}

// Op: 获取插件ID
#[op2]
#[string]
pub(in super::super) fn op_get_plugin_id(state: &mut OpState) -> String {
    state.borrow::<PluginOpState>().plugin_id.clone()
}
