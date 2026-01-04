use super::super::BotRuntime;
use axum::extract::{Json, Query};
use axum::Extension;

#[derive(serde::Deserialize)]
pub struct RelationsQuery {
    pub bot_id: String,
}

#[derive(serde::Deserialize)]
pub struct GroupMembersQuery {
    pub bot_id: String,
    pub group_id: u64,
}

#[derive(serde::Serialize)]
pub struct FriendInfo {
    pub user_id: u64,
    pub nickname: String,
    pub remark: String,
}

#[derive(serde::Serialize)]
pub struct GroupInfo {
    pub group_id: u64,
    pub group_name: String,
    pub member_count: u64,
}

#[derive(serde::Serialize)]
pub struct GroupMemberInfo {
    pub user_id: u64,
    pub nickname: String,
    pub card: String,
    pub role: String,
    pub join_time: u64,
    pub last_sent_time: u64,
}

#[derive(serde::Serialize)]
pub struct BotLoginInfo {
    pub user_id: u64,
    pub nickname: String,
}

pub async fn get_friends_handler(
    Extension(runtime): Extension<std::sync::Arc<BotRuntime>>,
    Query(query): Query<RelationsQuery>,
) -> Json<serde_json::Value> {
    let result = runtime
        .call_api(&query.bot_id, "get_friend_list", serde_json::json!({}))
        .await;

    match result {
        Some(resp) => {
            if let Some(data) = resp.get("data").and_then(|d| d.as_array()) {
                let friends: Vec<FriendInfo> = data
                    .iter()
                    .filter_map(|f| {
                        Some(FriendInfo {
                            user_id: f.get("user_id").and_then(|v| v.as_u64())?,
                            nickname: f
                                .get("nickname")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            remark: f
                                .get("remark")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                        })
                    })
                    .collect();
                Json(serde_json::json!({ "status": "success", "friends": friends }))
            } else {
                Json(serde_json::json!({
                    "status": "error",
                    "message": "Invalid response format from OneBot API"
                }))
            }
        }
        None => Json(serde_json::json!({
            "status": "error",
            "message": "Bot not connected (please login / wait for connection)"
        })),
    }
}

pub async fn get_groups_handler(
    Extension(runtime): Extension<std::sync::Arc<BotRuntime>>,
    Query(query): Query<RelationsQuery>,
) -> Json<serde_json::Value> {
    let result = runtime
        .call_api(&query.bot_id, "get_group_list", serde_json::json!({}))
        .await;

    match result {
        Some(resp) => {
            if let Some(data) = resp.get("data").and_then(|d| d.as_array()) {
                let groups: Vec<GroupInfo> = data
                    .iter()
                    .filter_map(|g| {
                        Some(GroupInfo {
                            group_id: g.get("group_id").and_then(|v| v.as_u64())?,
                            group_name: g
                                .get("group_name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            member_count: g
                                .get("member_count")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0),
                        })
                    })
                    .collect();
                Json(serde_json::json!({ "status": "success", "groups": groups }))
            } else {
                Json(serde_json::json!({
                    "status": "error",
                    "message": "Invalid response format from OneBot API"
                }))
            }
        }
        None => Json(serde_json::json!({
            "status": "error",
            "message": "Bot not connected (please login / wait for connection)"
        })),
    }
}

pub async fn get_group_members_handler(
    Extension(runtime): Extension<std::sync::Arc<BotRuntime>>,
    Query(query): Query<GroupMembersQuery>,
) -> Json<serde_json::Value> {
    let result = runtime
        .call_api(
            &query.bot_id,
            "get_group_member_list",
            serde_json::json!({ "group_id": query.group_id }),
        )
        .await;

    match result {
        Some(resp) => {
            if let Some(data) = resp.get("data").and_then(|d| d.as_array()) {
                let members: Vec<GroupMemberInfo> = data
                    .iter()
                    .filter_map(|m| {
                        Some(GroupMemberInfo {
                            user_id: m.get("user_id").and_then(|v| v.as_u64())?,
                            nickname: m
                                .get("nickname")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            card: m
                                .get("card")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            role: m
                                .get("role")
                                .and_then(|v| v.as_str())
                                .unwrap_or("member")
                                .to_string(),
                            join_time: m.get("join_time").and_then(|v| v.as_u64()).unwrap_or(0),
                            last_sent_time: m
                                .get("last_sent_time")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0),
                        })
                    })
                    .collect();
                Json(serde_json::json!({ "status": "success", "members": members }))
            } else {
                Json(serde_json::json!({
                    "status": "error",
                    "message": "Invalid response format from OneBot API"
                }))
            }
        }
        None => Json(serde_json::json!({
            "status": "error",
            "message": "Bot not connected (please login / wait for connection)"
        })),
    }
}

pub async fn get_login_info_handler(
    Extension(runtime): Extension<std::sync::Arc<BotRuntime>>,
    Query(query): Query<RelationsQuery>,
) -> Json<serde_json::Value> {
    let result = runtime
        .call_api(&query.bot_id, "get_login_info", serde_json::json!({}))
        .await;

    match result {
        Some(resp) => {
            if let Some(data) = resp.get("data") {
                let info = BotLoginInfo {
                    user_id: data.get("user_id").and_then(|v| v.as_u64()).unwrap_or(0),
                    nickname: data
                        .get("nickname")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                };
                Json(serde_json::json!({ "status": "success", "info": info }))
            } else {
                Json(serde_json::json!({
                    "status": "error",
                    "message": "Invalid response format from OneBot API"
                }))
            }
        }
        None => Json(serde_json::json!({
            "status": "error",
            "message": "Bot not connected (please login / wait for connection)"
        })),
    }
}
