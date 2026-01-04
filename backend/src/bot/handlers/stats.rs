use crate::models::SharedState;
use axum::extract::{Json, State};
use std::sync::atomic::Ordering;
use std::time::SystemTime;
use sysinfo::System;

static START_TIME: std::sync::OnceLock<SystemTime> = std::sync::OnceLock::new();

#[derive(serde::Serialize)]
pub struct SystemStats {
    pub cpu_usage: f32,
    pub memory_usage: f32,
    pub disk_usage: f32,
}

pub async fn get_system_stats_handler() -> Json<SystemStats> {
    let mut sys = System::new_all();
    sys.refresh_all();

    let cpu_usage = sys.global_cpu_usage();

    let total_mem = sys.total_memory();
    let used_mem = sys.used_memory();
    let memory_usage = if total_mem > 0 {
        (used_mem as f64 / total_mem as f64 * 100.0) as f32
    } else {
        0.0
    };

    let disks = sysinfo::Disks::new_with_refreshed_list();
    let (total_disk, used_disk) = disks.iter().fold((0u64, 0u64), |(t, u), d| {
        (
            t + d.total_space(),
            u + (d.total_space() - d.available_space()),
        )
    });
    let disk_usage = if total_disk > 0 {
        (used_disk as f64 / total_disk as f64 * 100.0) as f32
    } else {
        0.0
    };

    Json(SystemStats {
        cpu_usage,
        memory_usage,
        disk_usage,
    })
}

#[derive(serde::Serialize)]
pub struct MessageStatsResponse {
    pub total_messages: u64,
    pub today_messages: u64,
    pub total_calls: u64,
    pub today_calls: u64,
}

pub async fn get_message_stats_handler(
    State(state): State<SharedState>,
) -> Json<MessageStatsResponse> {
    state.message_stats.check_reset().await;
    Json(MessageStatsResponse {
        total_messages: state.message_stats.total_messages.load(Ordering::Relaxed),
        today_messages: state.message_stats.today_messages.load(Ordering::Relaxed),
        total_calls: state.message_stats.total_calls.load(Ordering::Relaxed),
        today_calls: state.message_stats.today_calls.load(Ordering::Relaxed),
    })
}

// System Info endpoint
#[derive(serde::Serialize)]
pub struct SystemInfo {
    pub version: String,
    pub rust_version: String,
    pub os: String,
    pub arch: String,
    pub data_dir: String,
    pub uptime_secs: u64,
}

pub async fn get_system_info_handler() -> Json<SystemInfo> {
    let start = START_TIME.get_or_init(SystemTime::now);
    let uptime_secs = start.elapsed().map(|d| d.as_secs()).unwrap_or(0);

    let data_dir = std::env::var("NBOT_DATA_DIR").unwrap_or_else(|_| "data".to_string());

    Json(SystemInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        rust_version: rustc_version_runtime::version().to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        data_dir,
        uptime_secs,
    })
}

// Docker Info endpoint
#[derive(serde::Serialize)]
pub struct DockerInfo {
    pub available: bool,
    pub version: String,
    pub containers_running: u32,
    pub containers_total: u32,
}

pub async fn get_docker_info_handler() -> Json<DockerInfo> {
    // Check docker version
    let version = match tokio::process::Command::new("docker")
        .args(["version", "--format", "{{.Server.Version}}"])
        .output()
        .await
    {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => String::new(),
    };

    let available = !version.is_empty();

    // Get container counts
    let (containers_running, containers_total) = if available {
        let running = tokio::process::Command::new("docker")
            .args(["ps", "-q"])
            .output()
            .await
            .map(|o| o.stdout.split(|&b| b == b'\n').filter(|l| !l.is_empty()).count() as u32)
            .unwrap_or(0);

        let total = tokio::process::Command::new("docker")
            .args(["ps", "-aq"])
            .output()
            .await
            .map(|o| o.stdout.split(|&b| b == b'\n').filter(|l| !l.is_empty()).count() as u32)
            .unwrap_or(0);

        (running, total)
    } else {
        (0, 0)
    };

    Json(DockerInfo {
        available,
        version,
        containers_running,
        containers_total,
    })
}

// System Export endpoint
pub async fn system_export_handler() -> axum::response::Response {
    use axum::http::{header, StatusCode};
    use axum::response::IntoResponse;

    let data_dir = std::env::var("NBOT_DATA_DIR").unwrap_or_else(|_| "data".to_string());
    let data_path = std::path::Path::new(&data_dir);

    // Create a JSON export of configuration
    let mut export_data = serde_json::Map::new();

    // Export bots config
    let bots_path = data_path.join("state").join("bots.json");
    if let Ok(content) = tokio::fs::read_to_string(&bots_path).await {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            export_data.insert("bots".to_string(), json);
        }
    }

    // Export databases config
    let dbs_path = data_path.join("state").join("databases.json");
    if let Ok(content) = tokio::fs::read_to_string(&dbs_path).await {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            export_data.insert("databases".to_string(), json);
        }
    }

    // Export modules config
    let modules_path = data_path.join("modules");
    if modules_path.exists() {
        let mut modules = serde_json::Map::new();
        if let Ok(mut entries) = tokio::fs::read_dir(&modules_path).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.extension().map(|e| e == "json").unwrap_or(false) {
                    if let Ok(content) = tokio::fs::read_to_string(&path).await {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                            if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                                modules.insert(name.to_string(), json);
                            }
                        }
                    }
                }
            }
        }
        if !modules.is_empty() {
            export_data.insert("modules".to_string(), serde_json::Value::Object(modules));
        }
    }

    // Export plugins config
    let plugins_path = data_path.join("plugins");
    if plugins_path.exists() {
        let mut plugins = serde_json::Map::new();
        if let Ok(mut entries) = tokio::fs::read_dir(&plugins_path).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.is_dir() {
                    let manifest_path = path.join("manifest.json");
                    if let Ok(content) = tokio::fs::read_to_string(&manifest_path).await {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                                plugins.insert(name.to_string(), json);
                            }
                        }
                    }
                }
            }
        }
        if !plugins.is_empty() {
            export_data.insert("plugins".to_string(), serde_json::Value::Object(plugins));
        }
    }

    let json_str = match serde_json::to_string_pretty(&serde_json::Value::Object(export_data)) {
        Ok(s) => s,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let filename = format!("nbot_export_{}.json", timestamp);

    (
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CONTENT_DISPOSITION, &format!("attachment; filename=\"{}\"", filename)),
        ],
        json_str,
    ).into_response()
}
