use crate::models::ContainerInfo;
use axum::extract::{Json, Query};
use std::collections::HashMap;
use tokio::process::Command;
use tracing::{error, info};

pub async fn list_containers_handler() -> Json<Vec<ContainerInfo>> {
    let output = Command::new("docker")
        .args([
            "ps",
            "-a",
            "--format",
            "{{.ID}}|{{.Names}}|{{.Status}}|{{.Image}}|{{.CreatedAt}}",
        ])
        .output()
        .await;

    let mut containers = Vec::new();
    if let Ok(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 5 {
                let name = parts[1].to_lowercase();
                if name.contains("napcat") || name.contains("nbot") || name.contains("bot") {
                    containers.push(ContainerInfo {
                        id: parts[0].to_string(),
                        name: parts[1].to_string(),
                        status: parts[2].to_string(),
                        image: parts[3].to_string(),
                        created: parts[4].to_string(),
                    });
                }
            }
        }
    }
    Json(containers)
}

#[derive(serde::Deserialize)]
pub struct ContainerAction {
    pub id: String,
    pub action: String,
}

pub async fn container_action_handler(
    Json(payload): Json<ContainerAction>,
) -> Json<serde_json::Value> {
    info!("Docker 操作: {} 于 {}", payload.action, payload.id);
    let output = Command::new("docker")
        .args([&payload.action, &payload.id])
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => Json(serde_json::json!({ "status": "success" })),
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr);
            error!("Docker 错误: {}", err);
            Json(serde_json::json!({ "status": "error", "message": err }))
        }
        Err(e) => {
            error!("执行 docker 失败: {}", e);
            Json(serde_json::json!({ "status": "error", "message": e.to_string() }))
        }
    }
}

pub async fn container_logs_handler(
    Query(params): Query<HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let container_id = match params.get("id").map(|s| s.trim()).filter(|s| !s.is_empty()) {
        Some(v) => v,
        None => {
            return Json(serde_json::json!({ "logs": "Error: missing query param `id`" }));
        }
    };

    let output = Command::new("docker")
        .args(["logs", "--tail", "200", container_id])
        .output()
        .await;

    match output {
        Ok(out) => {
            let mut logs = String::from_utf8_lossy(&out.stdout).to_string();
            logs.push_str(&String::from_utf8_lossy(&out.stderr));

            let clean_bytes = strip_ansi_escapes::strip(logs.as_bytes());
            let clean_logs = String::from_utf8_lossy(&clean_bytes);
            Json(serde_json::json!({ "logs": clean_logs }))
        }
        Err(e) => Json(serde_json::json!({ "logs": format!("Error: {}", e) })),
    }
}
