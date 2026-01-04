use crate::models::SharedState;
use futures_util::{SinkExt, StreamExt};
use reqwest::Client as HttpClient;
use serde_json::Value;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::sync::{mpsc, oneshot, watch, Mutex};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::protocol::Message;
use tracing::{info, warn};

use super::message::handle_event;

pub type WsSender = mpsc::UnboundedSender<String>;
pub type ResponseSender = oneshot::Sender<Value>;

const GROUP_SEND_STATUS_TTL: Duration = Duration::from_secs(3);
const DISCORD_MSG_INDEX_MAX: usize = 2048;

#[derive(Debug, Clone)]
pub enum GroupSendStatus {
    Allowed,
    Muted,
    Unknown,
}

#[derive(Debug, Clone)]
struct CachedGroupSendStatus {
    checked_at: Instant,
    status: GroupSendStatus,
}

#[derive(Clone)]
pub struct DiscordConnection {
    pub token: Arc<String>,
    pub http: HttpClient,
    pub shutdown: watch::Sender<bool>,
}

#[derive(Clone)]
pub enum BotConnection {
    OneBot { sender: WsSender },
    Discord(DiscordConnection),
}

#[derive(Debug, Clone)]
struct IndexedDiscordMessage {
    data: Value,
}

/// 消息去重缓存，防止网络恢复时重复发送
pub struct MessageDedup {
    cache: HashMap<u64, Instant>,
    ttl_secs: u64,
}

impl MessageDedup {
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            cache: HashMap::new(),
            ttl_secs,
        }
    }

    /// 检查消息是否重复，如果不重复则记录并返回 false
    pub fn is_duplicate(&mut self, hash: u64) -> bool {
        let now = Instant::now();
        // 清理过期条目
        self.cache
            .retain(|_, t| now.duration_since(*t).as_secs() < self.ttl_secs);

        if let Some(last) = self.cache.get(&hash) {
            if now.duration_since(*last).as_secs() < self.ttl_secs {
                return true; // 重复
            }
        }
        self.cache.insert(hash, now);
        false
    }
}

pub struct BotRuntime {
    pub connections: Arc<RwLock<HashMap<String, BotConnection>>>,
    pub pending_requests: Arc<RwLock<HashMap<String, ResponseSender>>>,
    pub message_dedup: Arc<Mutex<MessageDedup>>,
    self_id_cache: Arc<RwLock<HashMap<String, u64>>>,
    group_send_status_cache: Arc<Mutex<HashMap<(String, u64), CachedGroupSendStatus>>>,
    discord_msg_index: Arc<Mutex<HashMap<(String, u64), IndexedDiscordMessage>>>,
    discord_msg_fifo: Arc<Mutex<VecDeque<(String, u64)>>>,
}

impl BotRuntime {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            pending_requests: Arc::new(RwLock::new(HashMap::new())),
            message_dedup: Arc::new(Mutex::new(MessageDedup::new(5))), // 5秒去重窗口
            self_id_cache: Arc::new(RwLock::new(HashMap::new())),
            group_send_status_cache: Arc::new(Mutex::new(HashMap::new())),
            discord_msg_index: Arc::new(Mutex::new(HashMap::new())),
            discord_msg_fifo: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub async fn register_discord_connection(&self, bot_id: &str, conn: DiscordConnection) {
        self.connections
            .write()
            .await
            .insert(bot_id.to_string(), BotConnection::Discord(conn));
    }

    pub async fn unregister_connection(&self, bot_id: &str) {
        self.connections.write().await.remove(bot_id);
    }

    pub async fn shutdown_discord_connection(&self, bot_id: &str) {
        let conn = self.connections.read().await.get(bot_id).cloned();
        if let Some(BotConnection::Discord(conn)) = conn {
            let _ = conn.shutdown.send(true);
        }
        self.unregister_connection(bot_id).await;
    }

    pub async fn set_self_id(&self, bot_id: &str, user_id: u64) {
        if user_id == 0 {
            return;
        }
        self.self_id_cache
            .write()
            .await
            .insert(bot_id.to_string(), user_id);
    }

    pub async fn cache_group_send_status(
        &self,
        bot_id: &str,
        group_id: u64,
        status: GroupSendStatus,
    ) {
        if group_id == 0 {
            return;
        }
        self.group_send_status_cache.lock().await.insert(
            (bot_id.to_string(), group_id),
            CachedGroupSendStatus {
                checked_at: Instant::now(),
                status,
            },
        );
    }

    pub async fn index_discord_message(&self, bot_id: &str, message_id: u64, data: Value) {
        let key = (bot_id.to_string(), message_id);
        {
            let mut index = self.discord_msg_index.lock().await;
            index.insert(key.clone(), IndexedDiscordMessage { data });
        }

        let mut fifo = self.discord_msg_fifo.lock().await;
        fifo.push_back(key);
        while fifo.len() > DISCORD_MSG_INDEX_MAX {
            if let Some(old) = fifo.pop_front() {
                self.discord_msg_index.lock().await.remove(&old);
            }
        }
    }

    /// 调用 OneBot API 并等待响应
    pub async fn call_api(&self, bot_id: &str, action: &str, params: Value) -> Option<Value> {
        let conn = self.connections.read().await.get(bot_id).cloned();
        match conn {
            Some(BotConnection::Discord(_)) => {
                return self.call_discord_api(bot_id, action, params).await;
            }
            Some(BotConnection::OneBot { .. }) => {}
            None => return None,
        }

        let now_nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let echo = format!("{}_{}", action, now_nanos);

        let msg = serde_json::json!({
            "action": action,
            "params": params,
            "echo": echo
        });

        info!("发送 API 请求: action={}, echo={}", action, echo);

        // 创建响应通道
        let (tx, rx) = oneshot::channel();
        self.pending_requests.write().await.insert(echo.clone(), tx);

        // 发送请求
        let conns = self.connections.read().await;
        let sender = match conns.get(bot_id) {
            Some(BotConnection::OneBot { sender }) => sender,
            _ => {
                self.pending_requests.write().await.remove(&echo);
                return None;
            }
        };
        if sender.send(msg.to_string()).is_err() {
            self.pending_requests.write().await.remove(&echo);
            return None;
        }
        drop(conns);

        // 等待响应（超时 15 秒）
        match tokio::time::timeout(std::time::Duration::from_secs(15), rx).await {
            Ok(Ok(response)) => Some(response),
            _ => {
                self.pending_requests.write().await.remove(&echo);
                None
            }
        }
    }

    pub async fn get_self_id(&self, bot_id: &str) -> Option<u64> {
        if let Some(id) = self.self_id_cache.read().await.get(bot_id).copied() {
            return Some(id);
        }

        if matches!(
            self.connections.read().await.get(bot_id),
            Some(BotConnection::Discord(_))
        ) {
            return None;
        }

        let resp = self
            .call_api(bot_id, "get_login_info", serde_json::json!({}))
            .await?;
        let data = resp.get("data").unwrap_or(&resp);
        let user_id = data.get("user_id").and_then(|v| v.as_u64()).or_else(|| {
            data.get("user_id")
                .and_then(|v| v.as_str()?.parse::<u64>().ok())
        })?;

        if user_id == 0 {
            return None;
        }

        self.self_id_cache
            .write()
            .await
            .insert(bot_id.to_string(), user_id);
        Some(user_id)
    }

    pub async fn get_group_send_status(&self, bot_id: &str, group_id: u64) -> GroupSendStatus {
        if group_id == 0 {
            return GroupSendStatus::Allowed;
        }

        // Discord: group_id 实际上是 channel_id，不强制调用 “群成员/群信息” API；
        // 发送权限由 send_api 的错误回写缓存决定。
        if matches!(
            self.connections.read().await.get(bot_id),
            Some(BotConnection::Discord(_))
        ) {
            let key = (bot_id.to_string(), group_id);
            let cache = self.group_send_status_cache.lock().await;
            if let Some(entry) = cache.get(&key) {
                if entry.checked_at.elapsed() < GROUP_SEND_STATUS_TTL {
                    return entry.status.clone();
                }
            }
            return GroupSendStatus::Allowed;
        }

        let key = (bot_id.to_string(), group_id);
        {
            let cache = self.group_send_status_cache.lock().await;
            if let Some(entry) = cache.get(&key) {
                if entry.checked_at.elapsed() < GROUP_SEND_STATUS_TTL {
                    return entry.status.clone();
                }
            }
        }

        let self_id = match self.get_self_id(bot_id).await {
            Some(id) => id,
            None => return GroupSendStatus::Unknown,
        };

        let member_resp = match self
            .call_api(
                bot_id,
                "get_group_member_info",
                serde_json::json!({
                    "group_id": group_id,
                    "user_id": self_id,
                    "no_cache": true,
                }),
            )
            .await
        {
            Some(v) => v,
            None => return GroupSendStatus::Unknown,
        };

        if member_resp.get("status").and_then(|s| s.as_str()) != Some("ok") {
            return GroupSendStatus::Unknown;
        }

        let member_data = member_resp.get("data").unwrap_or(&member_resp);

        let shut_up_raw = member_data
            .get("shut_up_timestamp")
            .and_then(|v| v.as_u64())
            .or_else(|| {
                member_data
                    .get("shut_up_timestamp")
                    .and_then(|v| v.as_str()?.parse::<u64>().ok())
            })
            .unwrap_or(0);

        let role = member_data
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let is_privileged = role == "admin" || role == "owner";

        let mut status = GroupSendStatus::Allowed;

        if shut_up_raw > 0 {
            status = GroupSendStatus::Muted;
        } else if !is_privileged {
            if let Some(group_resp) = self
                .call_api(
                    bot_id,
                    "get_group_info",
                    serde_json::json!({
                        "group_id": group_id,
                        "no_cache": true,
                    }),
                )
                .await
            {
                if group_resp.get("status").and_then(|s| s.as_str()) == Some("ok") {
                    let group_data = group_resp.get("data").unwrap_or(&group_resp);
                    let all_shut = match group_data.get("group_all_shut") {
                        Some(Value::Bool(b)) => *b,
                        Some(Value::Number(n)) => n.as_i64().map(|x| x != 0).unwrap_or(false),
                        Some(Value::String(s)) => s.parse::<i64>().map(|x| x != 0).unwrap_or(false),
                        _ => false,
                    };
                    if all_shut {
                        status = GroupSendStatus::Muted;
                    }
                }
            }
        }

        self.group_send_status_cache.lock().await.insert(
            key,
            CachedGroupSendStatus {
                checked_at: Instant::now(),
                status: status.clone(),
            },
        );

        status
    }

    async fn call_discord_api(&self, bot_id: &str, action: &str, params: Value) -> Option<Value> {
        match action {
            "get_login_info" => {
                let self_id = self.self_id_cache.read().await.get(bot_id).copied()?;
                Some(serde_json::json!({
                    "status": "ok",
                    "data": { "user_id": self_id }
                }))
            }
            "get_msg" => {
                let message_id = params
                    .get("message_id")
                    .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse::<u64>().ok()))?;
                let key = (bot_id.to_string(), message_id);
                let idx = self.discord_msg_index.lock().await;
                let msg = idx.get(&key)?.clone();
                Some(serde_json::json!({
                    "status": "ok",
                    "data": msg.data
                }))
            }
            _ => None,
        }
    }
}

/// 为每个已连接的 bot 启动持久 WebSocket 连接
pub async fn start_bot_connections(state: SharedState, runtime: Arc<BotRuntime>) {
    info!("启动 Bot 消息监听服务...");

    loop {
        let bots_to_connect: Vec<(String, String, u16)> = state
            .bots
            .iter()
            .filter(|b| b.is_connected && b.ws_port.is_some())
            .filter_map(|b| {
                b.ws_port.map(|p| {
                    (
                        b.id.clone(),
                        b.ws_host.clone().unwrap_or_else(|| "127.0.0.1".to_string()),
                        p,
                    )
                })
            })
            .collect();

        for (bot_id, host, port) in bots_to_connect {
            let has_connection = runtime.connections.read().await.contains_key(&bot_id);
            if has_connection {
                continue;
            }

            let state_cl = state.clone();
            let runtime_cl = runtime.clone();
            let bot_id_cl = bot_id.clone();
            let host_cl = host.clone();

            tokio::spawn(async move {
                run_bot_connection(state_cl, runtime_cl, bot_id_cl, host_cl, port).await;
            });
        }

        // 清理已断开的连接
        let disconnected: Vec<String> = {
            let conns = runtime.connections.read().await;
            state
                .bots
                .iter()
                .filter(|b| {
                    if b.is_connected {
                        return false;
                    }
                    matches!(conns.get(&b.id), Some(BotConnection::OneBot { .. }))
                })
                .map(|b| b.id.clone())
                .collect()
        };

        for bot_id in disconnected {
            runtime.connections.write().await.remove(&bot_id);
            info!("已清理 {} 的连接", bot_id);
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
}

async fn run_bot_connection(
    state: SharedState,
    runtime: Arc<BotRuntime>,
    bot_id: String,
    host: String,
    port: u16,
) {
    let url = format!("ws://{}:{}", host, port);
    info!("建立 {} 的持久连接: {}", bot_id, url);

    match connect_async(&url).await {
        Ok((ws_stream, _)) => {
            let (mut write, mut read) = ws_stream.split();
            let (tx, mut rx) = mpsc::unbounded_channel::<String>();

            runtime
                .connections
                .write()
                .await
                .insert(bot_id.clone(), BotConnection::OneBot { sender: tx });
            info!("{} 已建立持久连接", bot_id);

            // 发送任务
            let bot_id_send = bot_id.clone();
            let send_task = tokio::spawn(async move {
                while let Some(msg) = rx.recv().await {
                    if write.send(Message::Text(msg)).await.is_err() {
                        warn!("{} 发送失败", bot_id_send);
                        break;
                    }
                }
            });

            // 接收任务
            let bot_id_recv = bot_id.clone();

            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        if let Ok(event) = serde_json::from_str::<Value>(&text) {
                            // API 响应直接处理（不阻塞接收循环）
                            if event.get("echo").is_some() {
                                if let Some(echo) = event.get("echo") {
                                    info!("[{}] 收到 WS 响应: echo={}", bot_id_recv, echo);
                                    let echo_str = if let Some(s) = echo.as_str() {
                                        s.to_string()
                                    } else {
                                        echo.to_string().trim_matches('"').to_string()
                                    };
                                    if let Some(sender) =
                                        runtime.pending_requests.write().await.remove(&echo_str)
                                    {
                                        let _ = sender.send(event);
                                    }
                                }
                            } else {
                                // 其他事件异步处理，避免阻塞接收循环
                                let state_cl = state.clone();
                                let runtime_cl = runtime.clone();
                                let bot_id_cl = bot_id_recv.clone();
                                tokio::spawn(async move {
                                    handle_event(&state_cl, &runtime_cl, &bot_id_cl, event).await;
                                });
                            }
                        }
                    }
                    Ok(Message::Close(_)) => {
                        info!("{} 连接已关闭", bot_id_recv);
                        break;
                    }
                    Err(e) => {
                        warn!("{} 接收错误: {:?}", bot_id_recv, e);
                        break;
                    }
                    _ => {}
                }
            }

            send_task.abort();
            runtime.unregister_connection(&bot_id).await;
            info!("{} 连接已断开", bot_id);
        }
        Err(e) => {
            // 连接失败时静默处理，napcat_login_monitor 会检测登录状态
            warn!("连接 {} 失败: {:?}", bot_id, e);
        }
    }
}
