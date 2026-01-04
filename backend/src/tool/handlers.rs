use axum::extract::State;
use axum::Json;
use std::sync::Arc;
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use tracing::{info, warn};

use super::{infra_from_compose_service, tool_from_compose_service, ToolContainer};
use crate::models::AppState;

type SharedState = Arc<AppState>;

/// 获取项目根目录（docker-compose.yml 所在目录）
fn get_project_root() -> std::path::PathBuf {
    std::env::var("PROJECT_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| ".".into()))
}

fn compose_project_name() -> String {
    std::env::var("COMPOSE_PROJECT_NAME")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "nbot".to_string())
}

fn docker_compose_command(project_root: &std::path::Path) -> Command {
    let mut cmd = Command::new("docker");
    cmd.arg("compose");
    cmd.arg("--project-name");
    cmd.arg(compose_project_name());
    cmd.current_dir(project_root);
    cmd
}

fn truncate_output(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }
    let mut out = s[s.len() - max_len..].to_string();
    out.insert_str(0, "...(truncated)...\n");
    out
}

async fn discover_tools_from_compose(
    project_root: &std::path::Path,
) -> Result<Vec<ToolContainer>, String> {
    let output = docker_compose_command(project_root)
        .args(["config", "--format", "json"])
        .output()
        .await
        .map_err(|e| format!("无法执行 docker compose config: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "docker compose config 失败 (exit={}):\n{}\n{}",
            output.status.code().unwrap_or(-1),
            truncate_output(&stderr, 4000),
            truncate_output(&stdout, 4000)
        ));
    }

    let raw = String::from_utf8_lossy(&output.stdout).to_string();
    let cfg: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("解析 compose config JSON 失败: {}", e))?;

    let services = cfg
        .get("services")
        .and_then(|v| v.as_object())
        .ok_or_else(|| "compose config 缺少 services 字段".to_string())?;

    let mut tools = Vec::new();
    for (service_id, service) in services {
        if let Some(tool) = tool_from_compose_service(service_id, service) {
            tools.push(tool);
        }
        if let Some(infra) = infra_from_compose_service(service_id, service) {
            tools.push(infra);
        }
    }

    tools.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(tools)
}

/// 列出所有工具容器及其状态
pub async fn list_tools_handler(State(_state): State<SharedState>) -> Json<Vec<ToolContainer>> {
    let project_root = get_project_root();

    let mut tools = Vec::<ToolContainer>::new();

    match discover_tools_from_compose(&project_root).await {
        Ok(mut t) => tools.append(&mut t),
        Err(e) => warn!("发现工具容器失败: {}", e),
    }

    tools.extend(super::runners::runner_tools().await);
    tools.sort_by(|a, b| a.id.cmp(&b.id));

    // 检查每个工具容器的状态
    for tool in &mut tools {
        if tool.kind == "runner" {
            continue;
        }

        let container_id = get_compose_container_id(&tool.id).await;
        tool.container_id = container_id.clone();

        if let Some(id) = container_id {
            if let Some(name) = get_container_name_by_id(&id).await {
                tool.container_name = name;
            }
            tool.status = get_container_status_by_id(&id).await;
        } else {
            tool.status = "notfound".to_string();
        }
    }

    Json(tools)
}

/// 启动工具容器
pub async fn start_tool_handler(
    State(_state): State<SharedState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Json<serde_json::Value> {
    info!("启动工具容器: {}", id);

    if super::runners::is_runner_tool_id(&id) {
        return Json(
            serde_json::json!({ "status": "error", "message": "该工具为非常驻工具，不支持启动/停止；请使用拉取镜像功能" }),
        );
    }

    let project_root = get_project_root();

    // 使用 docker compose 启动（需要在项目根目录执行）
    let output = docker_compose_command(&project_root)
        .args(["up", "-d", &id])
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => {
            info!("工具容器 {} 启动成功", id);
            Json(serde_json::json!({ "status": "success", "message": "启动成功" }))
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            warn!("工具容器 {} 启动失败: {} {}", id, stderr, stdout);
            Json(
                serde_json::json!({ "status": "error", "message": format!("启动失败: {}", stderr) }),
            )
        }
        Err(e) => {
            warn!("无法执行 docker compose: {}", e);
            Json(serde_json::json!({ "status": "error", "message": format!("执行失败: {}", e) }))
        }
    }
}

/// 停止工具容器
pub async fn stop_tool_handler(
    State(_state): State<SharedState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Json<serde_json::Value> {
    info!("停止工具容器: {}", id);

    if super::runners::is_runner_tool_id(&id) {
        return Json(
            serde_json::json!({ "status": "error", "message": "该工具为非常驻工具，不支持启动/停止；请使用拉取镜像功能" }),
        );
    }

    let project_root = get_project_root();
    let output = docker_compose_command(&project_root)
        .args(["stop", &id])
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => {
            info!("工具容器 {} 已停止", id);
            Json(serde_json::json!({ "status": "success", "message": "已停止" }))
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            warn!("工具容器 {} 停止失败: {}", id, stderr);
            Json(
                serde_json::json!({ "status": "error", "message": format!("停止失败: {}", stderr) }),
            )
        }
        Err(e) => {
            Json(serde_json::json!({ "status": "error", "message": format!("执行失败: {}", e) }))
        }
    }
}

/// 重启工具容器
pub async fn restart_tool_handler(
    State(_state): State<SharedState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Json<serde_json::Value> {
    info!("重启工具容器: {}", id);

    if super::runners::is_runner_tool_id(&id) {
        return Json(
            serde_json::json!({ "status": "error", "message": "该工具为非常驻工具，不支持重启；请使用拉取镜像功能" }),
        );
    }

    let project_root = get_project_root();
    let output = docker_compose_command(&project_root)
        .args(["restart", &id])
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => {
            Json(serde_json::json!({ "status": "success", "message": "重启成功" }))
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            Json(
                serde_json::json!({ "status": "error", "message": format!("重启失败: {}", stderr) }),
            )
        }
        Err(e) => {
            Json(serde_json::json!({ "status": "error", "message": format!("执行失败: {}", e) }))
        }
    }
}

/// 重建工具容器（应用 docker-compose.yml 变更，强制 recreate）
pub async fn recreate_tool_handler(
    State(_state): State<SharedState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Json<serde_json::Value> {
    info!("重建工具容器: {}", id);

    if super::runners::is_runner_tool_id(&id) {
        return Json(
            serde_json::json!({ "status": "error", "message": "该工具为非常驻工具，不支持重建；请使用拉取镜像功能" }),
        );
    }

    let project_root = get_project_root();
    let mut cmd = docker_compose_command(&project_root);
    cmd.args(["up", "-d", "--no-deps", "--force-recreate", "--build", &id]);

    let output = timeout(Duration::from_secs(120), cmd.output()).await;

    match output {
        Ok(Ok(out)) if out.status.success() => Json(serde_json::json!({
            "status": "success",
            "message": "重建成功",
        })),
        Ok(Ok(out)) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            let msg = format!(
                "重建失败 (exit={}):\n{}\n{}",
                out.status.code().unwrap_or(-1),
                truncate_output(&stderr, 4000),
                truncate_output(&stdout, 4000),
            );
            warn!("工具容器 {} 重建失败: {}", id, msg);
            Json(serde_json::json!({ "status": "error", "message": msg }))
        }
        Ok(Err(e)) => {
            Json(serde_json::json!({ "status": "error", "message": format!("执行失败: {}", e) }))
        }
        Err(_) => Json(serde_json::json!({ "status": "error", "message": "重建超时（>120s）" })),
    }
}

/// 拉取工具镜像（用于非常驻工具，例如 ffmpeg）或 compose 工具的镜像
pub async fn pull_tool_handler(
    State(_state): State<SharedState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Json<serde_json::Value> {
    if id == "ffmpeg" {
        let image = super::runners::ffmpeg_image_for_pull();
        info!("拉取工具镜像 (ffmpeg): {}", image);

        let mut cmd = Command::new("docker");
        cmd.args(["pull", &image]);

        let output = timeout(Duration::from_secs(600), cmd.output()).await;
        return match output {
            Ok(Ok(out)) if out.status.success() => Json(serde_json::json!({
                "status": "success",
                "message": "镜像拉取成功",
            })),
            Ok(Ok(out)) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let stdout = String::from_utf8_lossy(&out.stdout);
                let msg = format!(
                    "镜像拉取失败 (exit={}):\n{}\n{}",
                    out.status.code().unwrap_or(-1),
                    truncate_output(&stderr, 4000),
                    truncate_output(&stdout, 4000),
                );
                warn!("工具镜像 ffmpeg 拉取失败: {}", msg);
                Json(serde_json::json!({ "status": "error", "message": msg }))
            }
            Ok(Err(e)) => Json(
                serde_json::json!({ "status": "error", "message": format!("执行失败: {}", e) }),
            ),
            Err(_) => {
                Json(serde_json::json!({ "status": "error", "message": "拉取超时（>600s）" }))
            }
        };
    }

    info!("拉取工具容器镜像: {}", id);

    let project_root = get_project_root();
    let mut cmd = docker_compose_command(&project_root);
    cmd.args(["pull", &id]);

    let output = timeout(Duration::from_secs(600), cmd.output()).await;
    match output {
        Ok(Ok(out)) if out.status.success() => Json(serde_json::json!({
            "status": "success",
            "message": "拉取成功",
        })),
        Ok(Ok(out)) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            let msg = format!(
                "拉取失败 (exit={}):\n{}\n{}",
                out.status.code().unwrap_or(-1),
                truncate_output(&stderr, 4000),
                truncate_output(&stdout, 4000),
            );
            warn!("工具 {} 拉取失败: {}", id, msg);
            Json(serde_json::json!({ "status": "error", "message": msg }))
        }
        Ok(Err(e)) => {
            Json(serde_json::json!({ "status": "error", "message": format!("执行失败: {}", e) }))
        }
        Err(_) => Json(serde_json::json!({ "status": "error", "message": "拉取超时（>600s）" })),
    }
}

async fn get_compose_container_id(service: &str) -> Option<String> {
    let project_root = get_project_root();
    let output = docker_compose_command(&project_root)
        .args(["ps", "-aq", service])
        .output()
        .await
        .ok()?;

    if output.status.success() {
        let id = String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
        if id.is_empty() {
            None
        } else {
            Some(id)
        }
    } else {
        None
    }
}

async fn get_container_status_by_id(container_id: &str) -> String {
    let output = Command::new("docker")
        .args(["inspect", "--format", "{{.State.Status}}", container_id])
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => {
            let status = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if status == "running" {
                "running".to_string()
            } else {
                "stopped".to_string()
            }
        }
        _ => "notfound".to_string(),
    }
}

async fn get_container_name_by_id(container_id: &str) -> Option<String> {
    let output = Command::new("docker")
        .args(["inspect", "--format", "{{.Name}}", container_id])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&output.stdout)
        .trim()
        .trim_start_matches('/')
        .to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}
