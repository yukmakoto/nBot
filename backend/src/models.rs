use crate::command::CommandRegistry;
use crate::logs::LogStore;
use crate::module::ModuleRegistry;
use crate::plugin::{PluginManager, PluginRegistry};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

/// 消息统计
pub struct MessageStats {
    pub total_messages: AtomicU64,
    pub total_calls: AtomicU64,
    pub today_messages: AtomicU64,
    pub today_calls: AtomicU64,
    pub last_reset_date: RwLock<String>,
}

impl MessageStats {
    pub fn new() -> Self {
        Self {
            total_messages: AtomicU64::new(0),
            total_calls: AtomicU64::new(0),
            today_messages: AtomicU64::new(0),
            today_calls: AtomicU64::new(0),
            last_reset_date: RwLock::new(String::new()),
        }
    }

    pub async fn check_reset(&self) {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let mut last = self.last_reset_date.write().await;
        if *last != today {
            self.today_messages.store(0, Ordering::Relaxed);
            self.today_calls.store(0, Ordering::Relaxed);
            *last = today;
        }
    }

    pub fn inc_message(&self) {
        self.total_messages.fetch_add(1, Ordering::Relaxed);
        self.today_messages.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_call(&self) {
        self.total_calls.fetch_add(1, Ordering::Relaxed);
        self.today_calls.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotInstance {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub is_connected: bool,
    pub is_running: bool,
    #[serde(default)]
    pub container_id: Option<String>,
    #[serde(default)]
    pub ws_host: Option<String>,
    #[serde(default)]
    pub ws_port: Option<u16>,
    #[serde(default)]
    pub webui_host: Option<String>,
    #[serde(default)]
    pub webui_port: Option<u16>,
    #[serde(default)]
    pub webui_token: Option<String>,
    #[serde(default)]
    pub qq_id: Option<String>,
    #[serde(default)]
    pub linked_database: Option<String>,
    pub metadata: serde_json::Value,
    #[serde(default)]
    pub modules_config: std::collections::HashMap<String, BotModuleConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BotModuleConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseInstance {
    pub id: String,
    pub name: String,
    pub db_type: String,
    pub container_id: Option<String>,
    pub host_port: u16,
    pub internal_port: u16,
    pub username: String,
    pub password: String,
    pub database_name: String,
    pub is_running: bool,
}

pub struct RuntimeState {
    pub latest_qr: RwLock<Option<String>>,
    pub latest_qr_image: RwLock<Option<String>>,
}

// ===== Background Tasks (Long-running ops) =====

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Running,
    Success,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProgress {
    pub current: u32,
    pub total: u32,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTask {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub state: TaskState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<TaskProgress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub status: String,
    pub image: String,
    pub created: String,
}

pub type SharedState = Arc<AppState>;

pub struct AppState {
    pub bots: DashMap<String, BotInstance>,
    pub databases: DashMap<String, DatabaseInstance>,
    pub tasks: DashMap<String, BackgroundTask>,
    pub runtime: Arc<RuntimeState>,
    pub logs: Arc<LogStore>,
    pub plugins: Arc<PluginRegistry>,
    pub plugin_manager: Arc<PluginManager>,
    pub modules: Arc<ModuleRegistry>,
    pub commands: Arc<CommandRegistry>,
    pub message_stats: Arc<MessageStats>,
}
