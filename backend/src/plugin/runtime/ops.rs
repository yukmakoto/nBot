use deno_core::OpState;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use tracing::error;

use super::state::MediaBundleItem;
use super::{PluginOpState, PluginOutput};

mod core;
mod group;
mod http;
mod llm;
mod render;
mod storage;

pub(super) mod state {
    pub use super::super::state::ForwardNode;
}

pub(super) use core::*;
pub(super) use group::*;
pub(super) use http::*;
pub(super) use llm::*;
pub(super) use render::*;
pub(super) use storage::*;

fn log_json_parse_error(state: &OpState, op_name: &str, err: &serde_json::Error) {
    let plugin_id = state.borrow::<PluginOpState>().plugin_id.clone();
    error!(
        "[插件:{}] {} payload JSON 解析失败: {}",
        plugin_id, op_name, err
    );
}

fn push_reply(state: &mut OpState, user_id: i64, group_id: i64, content: &str) {
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

fn parse_payload_or_reply<T: DeserializeOwned>(
    state: &mut OpState,
    user_id: i64,
    group_id: i64,
    op_name: &str,
    payload_json: &str,
) -> Option<T> {
    match serde_json::from_str::<T>(payload_json) {
        Ok(v) => Some(v),
        Err(e) => {
            log_json_parse_error(&*state, op_name, &e);
            push_reply(state, user_id, group_id, "插件内部错误：参数解析失败");
            None
        }
    }
}

fn safe_storage_key_filename(key: &str) -> String {
    let key = key.trim();
    let is_safe = !key.is_empty()
        && key.len() <= 64
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.');

    if is_safe {
        key.to_string()
    } else {
        let mut hasher = Sha256::new();
        hasher.update(key.as_bytes());
        let hash = hasher.finalize();
        format!("key_{:x}", hash)
    }
}
