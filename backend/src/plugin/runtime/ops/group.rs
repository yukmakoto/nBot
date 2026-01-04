use deno_core::{op2, OpState};

use super::{PluginOpState, PluginOutput};

/// Op: Fetch group announcements (async, result returned via onGroupInfoResponse hook)
#[op2(fast)]
pub(in super::super) fn op_fetch_group_notice(
    state: &mut OpState,
    #[string] request_id: &str,
    #[bigint] group_id: i64,
) {
    state
        .borrow_mut::<PluginOpState>()
        .outputs
        .push(PluginOutput::FetchGroupNotice {
            request_id: request_id.to_string(),
            group_id: group_id as u64,
        });
}

/// Op: Fetch group message history (async, result returned via onGroupInfoResponse hook)
#[op2(fast)]
pub(in super::super) fn op_fetch_group_msg_history(
    state: &mut OpState,
    #[string] request_id: &str,
    #[bigint] group_id: i64,
    count: u32,
    #[bigint] message_seq: i64,
) {
    state
        .borrow_mut::<PluginOpState>()
        .outputs
        .push(PluginOutput::FetchGroupMsgHistory {
            request_id: request_id.to_string(),
            group_id: group_id as u64,
            count: if count > 0 { Some(count) } else { None },
            message_seq: if message_seq > 0 {
                Some(message_seq as u64)
            } else {
                None
            },
        });
}

/// Op: Fetch group files (async, result returned via onGroupInfoResponse hook)
#[op2(fast)]
pub(in super::super) fn op_fetch_group_files(
    state: &mut OpState,
    #[string] request_id: &str,
    #[bigint] group_id: i64,
    #[string] folder_id: &str,
) {
    state
        .borrow_mut::<PluginOpState>()
        .outputs
        .push(PluginOutput::FetchGroupFiles {
            request_id: request_id.to_string(),
            group_id: group_id as u64,
            folder_id: if folder_id.is_empty() {
                None
            } else {
                Some(folder_id.to_string())
            },
        });
}

/// Op: Fetch group file download URL (async, result returned via onGroupInfoResponse hook)
#[op2(fast)]
pub(in super::super) fn op_fetch_group_file_url(
    state: &mut OpState,
    #[string] request_id: &str,
    #[bigint] group_id: i64,
    #[string] file_id: &str,
    busid: u32,
) {
    state
        .borrow_mut::<PluginOpState>()
        .outputs
        .push(PluginOutput::FetchGroupFileUrl {
            request_id: request_id.to_string(),
            group_id: group_id as u64,
            file_id: file_id.to_string(),
            busid: if busid > 0 { Some(busid) } else { None },
        });
}

/// Op: Fetch friend list (async, result returned via onGroupInfoResponse hook)
#[op2(fast)]
pub(in super::super) fn op_fetch_friend_list(state: &mut OpState, #[string] request_id: &str) {
    state
        .borrow_mut::<PluginOpState>()
        .outputs
        .push(PluginOutput::FetchFriendList {
            request_id: request_id.to_string(),
        });
}

/// Op: Fetch group list (async, result returned via onGroupInfoResponse hook)
#[op2(fast)]
pub(in super::super) fn op_fetch_group_list(state: &mut OpState, #[string] request_id: &str) {
    state
        .borrow_mut::<PluginOpState>()
        .outputs
        .push(PluginOutput::FetchGroupList {
            request_id: request_id.to_string(),
        });
}

/// Op: Fetch group member list (async, result returned via onGroupInfoResponse hook)
#[op2(fast)]
pub(in super::super) fn op_fetch_group_member_list(
    state: &mut OpState,
    #[string] request_id: &str,
    #[bigint] group_id: i64,
) {
    state
        .borrow_mut::<PluginOpState>()
        .outputs
        .push(PluginOutput::FetchGroupMemberList {
            request_id: request_id.to_string(),
            group_id: group_id as u64,
        });
}

/// Op: Download file to cache directory (async, result returned via onGroupInfoResponse hook)
#[op2(fast)]
pub(in super::super) fn op_download_file(
    state: &mut OpState,
    #[string] request_id: &str,
    #[string] url: &str,
    thread_count: u32,
    #[string] headers_json: &str,
) {
    let headers: Option<Vec<String>> = if headers_json.is_empty() {
        None
    } else {
        serde_json::from_str(headers_json).ok()
    };

    state
        .borrow_mut::<PluginOpState>()
        .outputs
        .push(PluginOutput::DownloadFile {
            request_id: request_id.to_string(),
            url: url.to_string(),
            thread_count: if thread_count > 0 {
                Some(thread_count)
            } else {
                None
            },
            headers,
        });
}
