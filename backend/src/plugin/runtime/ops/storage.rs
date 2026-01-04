use deno_core::{op2, OpState};
use std::fs;
use std::path::PathBuf;

use super::PluginOpState;

// Op: 存储数据
#[op2(fast)]
pub(in super::super) fn op_storage_set(
    state: &mut OpState,
    #[string] key: &str,
    #[string] value: &str,
) -> bool {
    let plugin_id = state.borrow::<PluginOpState>().plugin_id.clone();
    let data_dir = state.borrow::<PluginOpState>().data_dir.clone();
    let storage_dir = PathBuf::from(&data_dir)
        .join("plugins")
        .join("storage")
        .join(plugin_id);
    if fs::create_dir_all(&storage_dir).is_err() {
        return false;
    }
    let file_path = storage_dir.join(format!("{}.json", super::safe_storage_key_filename(key)));
    fs::write(file_path, value).is_ok()
}

// Op: 读取数据
#[op2]
#[string]
pub(in super::super) fn op_storage_get(state: &mut OpState, #[string] key: &str) -> Option<String> {
    let plugin_id = state.borrow::<PluginOpState>().plugin_id.clone();
    let data_dir = state.borrow::<PluginOpState>().data_dir.clone();
    let file_path = PathBuf::from(&data_dir)
        .join("plugins")
        .join("storage")
        .join(plugin_id)
        .join(format!("{}.json", super::safe_storage_key_filename(key)));
    fs::read_to_string(file_path).ok()
}

// Op: 删除数据
#[op2(fast)]
pub(in super::super) fn op_storage_delete(state: &mut OpState, #[string] key: &str) -> bool {
    let plugin_id = state.borrow::<PluginOpState>().plugin_id.clone();
    let data_dir = state.borrow::<PluginOpState>().data_dir.clone();
    let file_path = PathBuf::from(&data_dir)
        .join("plugins")
        .join("storage")
        .join(plugin_id)
        .join(format!("{}.json", super::safe_storage_key_filename(key)));
    fs::remove_file(file_path).is_ok()
}
