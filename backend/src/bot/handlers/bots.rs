use crate::models::{BackgroundTask, BotInstance, SharedState, TaskProgress, TaskState};
use crate::persistence::save_bots;
use crate::{http::ApiError, http::ApiResult};
use axum::extract::{Json, Path, State};
use axum::Extension;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::info;

/// Validates that a filename contains only safe characters.
/// Allows: alphanumeric, underscore, hyphen, dot (but not leading dot)
fn is_safe_filename(filename: &str) -> bool {
    if filename.is_empty() || filename.starts_with('.') {
        return false;
    }
    filename.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

fn sanitize_bot_for_api(mut bot: BotInstance) -> BotInstance {
    if bot.platform.eq_ignore_ascii_case("discord") {
        if let Some(discord) = bot
            .metadata
            .get_mut("discord")
            .and_then(|v| v.as_object_mut())
        {
            discord.remove("token");
        }
    }
    bot
}

fn now_unix_secs() -> Result<u64, ApiError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| ApiError::internal(format!("System time error: {}", e)))
}

fn now_unix_secs_lossy() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn bot_dirs(bot_id: &str) -> Result<(PathBuf, PathBuf), ApiError> {
    let current_dir = std::env::current_dir()
        .map_err(|e| ApiError::internal(format!("Failed to get current dir: {}", e)))?;
    let base_path = current_dir.join("data").join("bots").join(bot_id);
    Ok((base_path.join("config"), base_path.join("qq")))
}

fn build_onebot11_config_json() -> Result<String, ApiError> {
    // NapCat reads OneBot config from `/app/napcat/config/onebot11.json` (or onebot11_<uin>.json).
    // Do NOT write this into `napcat.json` (that file is NapCat's base config and breaking it will crash/restart).
    let config = serde_json::json!({
        "network": {
            "httpServers": [],
            "httpSseServers": [],
            "httpClients": [],
            "websocketServers": [{
                "enable": true,
                "name": "ws",
                "host": "0.0.0.0",
                "port": 3001,
                "reportSelfMessage": false,
                "enableForcePushEvent": true,
                "messagePostFormat": "array",
                "token": "",
                "debug": false,
                "heartInterval": 30000
            }],
            "websocketClients": [],
            "plugins": []
        },
        "musicSignUrl": "",
        "enableLocalFile2Url": false,
        // Needed for parsing merged-forward messages (合并转发消息) so we can analyze attachments reliably.
        "parseMultMsg": true
    });

    serde_json::to_string_pretty(&config)
        .map_err(|e| ApiError::internal(format!("Failed to serialize OneBot config: {}", e)))
}

async fn ensure_docker_network() -> Result<(), ApiError> {
    // Check first to avoid treating "already exists" as success/failure ambiguity.
    let inspect = Command::new("docker")
        .args(["network", "inspect", "nbot_default"])
        .output()
        .await
        .map_err(|e| ApiError::internal(format!("Failed to run docker: {}", e)))?;
    if inspect.status.success() {
        return Ok(());
    }

    let create = Command::new("docker")
        .args(["network", "create", "nbot_default"])
        .output()
        .await
        .map_err(|e| ApiError::internal(format!("Failed to run docker: {}", e)))?;
    if !create.status.success() {
        let err = String::from_utf8_lossy(&create.stderr);
        return Err(ApiError::internal(format!(
            "Failed to create docker network nbot_default: {}",
            err
        )));
    }
    Ok(())
}

async fn ensure_docker_volume(volume: &str) -> Result<bool, ApiError> {
    let inspect = Command::new("docker")
        .args(["volume", "inspect", volume])
        .output()
        .await
        .map_err(|e| ApiError::internal(format!("Failed to run docker: {}", e)))?;
    if inspect.status.success() {
        return Ok(true);
    }

    let create = Command::new("docker")
        .args(["volume", "create", volume])
        .output()
        .await
        .map_err(|e| ApiError::internal(format!("Failed to run docker: {}", e)))?;

    if !create.status.success() {
        let err = String::from_utf8_lossy(&create.stderr);
        return Err(ApiError::internal(format!(
            "Failed to create docker volume {}: {}",
            volume, err
        )));
    }
    Ok(false)
}

fn normalize_registry_host(registry: &str) -> String {
    let mut r = registry.trim().to_string();
    if let Some(rest) = r.strip_prefix("http://") {
        r = rest.to_string();
    }
    if let Some(rest) = r.strip_prefix("https://") {
        r = rest.to_string();
    }
    while r.ends_with('/') {
        r.pop();
    }
    r
}

fn default_volume_init_image() -> String {
    // We need a helper image to write files into Docker volumes when running inside Docker.
    // Do NOT depend on `library/alpine` (often blocked by private registries). Use our own images by default.
    let registry = std::env::var("NBOT_DOCKER_REGISTRY")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "docker.nailed.dev".to_string());
    let namespace = std::env::var("NBOT_DOCKERHUB_NAMESPACE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "yukmakoto".to_string());
    let tag = std::env::var("NBOT_TAG")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "latest".to_string());

    format!(
        "{}/{}/nbot-render:{}",
        normalize_registry_host(&registry),
        namespace.trim(),
        tag.trim()
    )
}

async fn write_volume_file(volume: &str, filename: &str, content: &str) -> Result<(), ApiError> {
    // Validate filename to prevent command injection
    if !is_safe_filename(filename) {
        return Err(ApiError::bad_request(format!(
            "Invalid filename: {}. Only alphanumeric, underscore, hyphen, and dot are allowed.",
            filename
        )));
    }

    let helper_image = std::env::var("NBOT_VOLUME_INIT_IMAGE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(default_volume_init_image);

    let cmd = format!("cat > /data/{}", filename);
    let mut child = Command::new("docker")
        .args([
            "run",
            "--rm",
            "-i",
            "-v",
            &format!("{}:/data", volume),
            &helper_image,
            "sh",
            "-c",
            &cmd,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ApiError::internal(format!("Failed to run docker: {}", e)))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(content.as_bytes())
            .await
            .map_err(|e| ApiError::internal(format!("Failed to write volume content: {}", e)))?;
    }

    let out = child
        .wait_with_output()
        .await
        .map_err(|e| ApiError::internal(format!("Failed to wait docker: {}", e)))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(ApiError::internal(format!(
            "Failed to write {} into volume {}: {}",
            filename, volume, err
        )));
    }

    Ok(())
}

async fn run_napcat_container(
    bot_id: &str,
    config_mount_src: &str,
    qq_mount_src: &str,
) -> Result<(), ApiError> {
    let napcat_image = std::env::var("NBOT_NAPCAT_IMAGE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "docker.nailed.dev/mlikiowa/napcat-docker:latest".to_string());
    let network_alias = bot_id.replace('_', "-");

    let output = Command::new("docker")
        .args([
            "run",
            "-d",
            "--name",
            bot_id,
            "--restart",
            "always",
            "--network",
            "nbot_default",
            "--network-alias",
            &network_alias,
            "-p",
            "127.0.0.1::3001",
            "-p",
            "127.0.0.1::6099",
            "-v",
            &format!("{}:/app/napcat/config", config_mount_src),
            "-v",
            &format!("{}:/app/.config/QQ", qq_mount_src),
            "--label",
            "nbot.managed=true",
            "--label",
            &format!("nbot.bot_id={}", bot_id),
            "--label",
            "nbot.kind=napcat",
            &napcat_image,
        ])
        .output()
        .await
        .map_err(|e| ApiError::internal(format!("Failed to run docker: {}", e)))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(ApiError::internal(format!("Docker error: {}", err)));
    }

    Ok(())
}

fn image_registry_host(image: &str) -> Option<String> {
    let first = image.split('/').next()?.trim();
    if first.is_empty() {
        return None;
    }
    if first.contains('.') || first.contains(':') || first.eq_ignore_ascii_case("localhost") {
        Some(first.to_string())
    } else {
        None
    }
}

fn task_update(state: &SharedState, task_id: &str, f: impl FnOnce(&mut BackgroundTask)) {
    if let Some(mut task) = state.tasks.get_mut(task_id) {
        f(&mut task);
        task.updated_at = now_unix_secs_lossy();
    }
}

fn task_progress(state: &SharedState, task_id: &str, current: u32, total: u32, label: &str) {
    task_update(state, task_id, |t| {
        t.state = TaskState::Running;
        t.progress = Some(TaskProgress {
            current,
            total,
            label: label.to_string(),
        });
    });
}

fn task_detail(state: &SharedState, task_id: &str, detail: impl Into<String>) {
    let detail = detail.into();
    task_update(state, task_id, |t| {
        t.detail = Some(detail);
    });
}

fn task_fail(state: &SharedState, task_id: &str, error: impl Into<String>) {
    let error = error.into();
    task_update(state, task_id, |t| {
        t.state = TaskState::Error;
        t.error = Some(error);
    });
}

fn task_success(state: &SharedState, task_id: &str, result: serde_json::Value) {
    task_update(state, task_id, |t| {
        t.state = TaskState::Success;
        t.result = Some(result);
    });
}

async fn docker_image_size_bytes(image: &str) -> Result<Option<u64>, ApiError> {
    let out = Command::new("docker")
        .args(["image", "inspect", image, "--format", "{{.Size}}"])
        .output()
        .await
        .map_err(|e| ApiError::internal(format!("Failed to run docker: {}", e)))?;

    if !out.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let size = stdout.trim().parse::<u64>().ok();
    Ok(size)
}

fn fmt_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2}GB", b / GB)
    } else if b >= MB {
        format!("{:.1}MB", b / MB)
    } else if b >= KB {
        format!("{:.1}KB", b / KB)
    } else {
        format!("{}B", bytes)
    }
}

async fn ensure_docker_image(
    task_state: &SharedState,
    task_id: &str,
    image: &str,
) -> Result<(), ApiError> {
    if let Some(size) = docker_image_size_bytes(image).await? {
        task_detail(
            task_state,
            task_id,
            format!("NapCat 镜像已存在（{}）", fmt_bytes(size)),
        );
        return Ok(());
    }

    task_detail(
        task_state,
        task_id,
        "正在拉取 NapCat 镜像（首次可能较慢，请耐心等待）",
    );

    let out = Command::new("docker")
        .args(["pull", image])
        .output()
        .await
        .map_err(|e| ApiError::internal(format!("Failed to run docker: {}", e)))?;

    if out.status.success() {
        if let Some(size) = docker_image_size_bytes(image).await? {
            task_detail(
                task_state,
                task_id,
                format!("NapCat 镜像拉取完成（{}）", fmt_bytes(size)),
            );
        } else {
            task_detail(task_state, task_id, "NapCat 镜像拉取完成");
        }
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&out.stderr);
    let msg = stderr.trim();
    let auth_hint = msg.contains("unauthorized")
        || msg.contains("authentication required")
        || msg.contains("denied")
        || msg.contains("pull access denied");
    if auth_hint {
        if let Some(reg) = image_registry_host(image) {
            return Err(ApiError::bad_gateway(format!(
                "NapCat 镜像拉取失败：需要登录镜像仓库 {}。请先执行 docker login {} 后重试。",
                reg, reg
            )));
        }
        return Err(ApiError::bad_gateway(
            "NapCat 镜像拉取失败：需要登录镜像仓库。请先 docker login 后重试。".to_string(),
        ));
    }

    Err(ApiError::bad_gateway(format!(
        "NapCat 镜像拉取失败：{}",
        msg
    )))
}

async fn get_container_host_port(container: &str, internal_port: u16) -> Result<u16, ApiError> {
    let port_key = format!("{}/tcp", internal_port);

    for _ in 0..20 {
        let output = Command::new("docker")
            .args([
                "inspect",
                "--format",
                "{{json .NetworkSettings.Ports}}",
                container,
            ])
            .output()
            .await
            .map_err(|e| ApiError::internal(format!("Failed to run docker: {}", e)))?;

        if !output.status.success() {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            continue;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let Ok(ports) = serde_json::from_str::<serde_json::Value>(stdout.trim()) else {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            continue;
        };

        let Some(list) = ports.get(&port_key).and_then(|v| v.as_array()) else {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            continue;
        };

        let host_port = list
            .iter()
            .filter_map(|v| v.get("HostPort").and_then(|p| p.as_str()))
            .find_map(|s| s.trim().parse::<u16>().ok());

        if let Some(p) = host_port {
            return Ok(p);
        }

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    Err(ApiError::internal(format!(
        "Failed to get published port for {} ({})",
        container, port_key
    )))
}

async fn cleanup_failed_napcat_provision(
    bot_id: &str,
    base_dir: &std::path::Path,
    base_existed: bool,
) {
    let _ = Command::new("docker")
        .args(["rm", "-f", bot_id])
        .output()
        .await;
    if !base_existed {
        let _ = std::fs::remove_dir_all(base_dir);
    }
}

async fn cleanup_failed_napcat_provision_docker(bot_id: &str, volumes: &[(&str, bool)]) {
    let _ = Command::new("docker")
        .args(["rm", "-f", bot_id])
        .output()
        .await;

    for (vol, existed) in volumes {
        if *existed {
            continue;
        }
        let _ = Command::new("docker")
            .args(["volume", "rm", vol])
            .output()
            .await;
    }
}

struct ProvisionedBotContainer {
    ws_host: String,
    ws_port: u16,
    webui_host: String,
    webui_port: u16,
}

fn build_running_bot_instance(
    id: String,
    name: String,
    platform: String,
    provisioned: ProvisionedBotContainer,
    linked_database: Option<String>,
    metadata: serde_json::Value,
    modules_config: HashMap<String, crate::models::BotModuleConfig>,
) -> BotInstance {
    BotInstance {
        id: id.clone(),
        name,
        platform,
        is_connected: false,
        is_running: true,
        container_id: Some(id),
        ws_host: Some(provisioned.ws_host),
        ws_port: Some(provisioned.ws_port),
        webui_host: Some(provisioned.webui_host),
        webui_port: Some(provisioned.webui_port),
        webui_token: None,
        qq_id: None,
        linked_database,
        metadata,
        modules_config,
    }
}

async fn provision_napcat_bot_container(bot_id: &str) -> Result<ProvisionedBotContainer, ApiError> {
    let docker_mode = std::env::var("NBOT_DOCKER_MODE")
        .ok()
        .map(|v| {
            let v = v.trim();
            v.eq_ignore_ascii_case("1")
                || v.eq_ignore_ascii_case("true")
                || v.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false);

    ensure_docker_network().await?;

    let onebot11_json = build_onebot11_config_json()?;

    info!(
        "为机器人 {} 创建容器（{}）",
        bot_id,
        if docker_mode {
            "容器网络"
        } else {
            "宿主机随机端口映射"
        }
    );
    let (ws_host, ws_port, webui_host, webui_port) = if docker_mode {
        let alias = bot_id.replace('_', "-");
        let config_volume = format!("nbot-bot-{}-config", alias);
        let qq_volume = format!("nbot-bot-{}-qq", alias);

        let config_existed = ensure_docker_volume(&config_volume).await?;
        let qq_existed = ensure_docker_volume(&qq_volume).await?;
        let volumes = vec![
            (config_volume.as_str(), config_existed),
            (qq_volume.as_str(), qq_existed),
        ];

        if let Err(e) = write_volume_file(&config_volume, "onebot11.json", &onebot11_json).await {
            cleanup_failed_napcat_provision_docker(bot_id, &volumes).await;
            return Err(e);
        }

        if let Err(e) = run_napcat_container(bot_id, &config_volume, &qq_volume).await {
            cleanup_failed_napcat_provision_docker(bot_id, &volumes).await;
            return Err(e);
        }

        (alias.clone(), 3001, alias, 6099)
    } else {
        let (config_path, qq_path) = bot_dirs(bot_id)?;
        let Some(base_dir) = config_path.parent() else {
            return Err(ApiError::internal("Invalid bot config path".to_string()));
        };
        let base_existed = base_dir.exists();

        if let Err(e) = std::fs::create_dir_all(&config_path) {
            cleanup_failed_napcat_provision(bot_id, base_dir, base_existed).await;
            return Err(ApiError::internal(format!(
                "Failed to create config directory: {}",
                e
            )));
        }
        if let Err(e) = std::fs::create_dir_all(&qq_path) {
            cleanup_failed_napcat_provision(bot_id, base_dir, base_existed).await;
            return Err(ApiError::internal(format!(
                "Failed to create qq directory: {}",
                e
            )));
        }

        if let Err(e) = std::fs::write(config_path.join("onebot11.json"), &onebot11_json) {
            cleanup_failed_napcat_provision(bot_id, base_dir, base_existed).await;
            return Err(ApiError::internal(format!(
                "Failed to write OneBot config: {}",
                e
            )));
        }

        let config_mount = config_path.to_string_lossy().to_string();
        let qq_mount = qq_path.to_string_lossy().to_string();
        if let Err(e) = run_napcat_container(bot_id, &config_mount, &qq_mount).await {
            cleanup_failed_napcat_provision(bot_id, base_dir, base_existed).await;
            return Err(e);
        }

        let ws_port = match get_container_host_port(bot_id, 3001).await {
            Ok(p) => p,
            Err(e) => {
                cleanup_failed_napcat_provision(bot_id, base_dir, base_existed).await;
                return Err(e);
            }
        };
        let webui_port = match get_container_host_port(bot_id, 6099).await {
            Ok(p) => p,
            Err(e) => {
                cleanup_failed_napcat_provision(bot_id, base_dir, base_existed).await;
                return Err(e);
            }
        };

        (
            "127.0.0.1".to_string(),
            ws_port,
            "127.0.0.1".to_string(),
            webui_port,
        )
    };

    info!(
        "机器人 {} 已创建：WS={}://{}:{}, WebUI=http://{}:{}",
        bot_id, "ws", ws_host, ws_port, webui_host, webui_port
    );
    Ok(ProvisionedBotContainer {
        ws_host,
        ws_port,
        webui_host,
        webui_port,
    })
}

pub async fn get_status_handler(State(state): State<SharedState>) -> Json<Vec<BotInstance>> {
    let bots: Vec<BotInstance> = state
        .bots
        .iter()
        .map(|kv| sanitize_bot_for_api(kv.value().clone()))
        .collect();
    Json(bots)
}

#[derive(serde::Deserialize)]
pub struct CreateBotPayload {
    pub name: String,
    pub platform: String,
}

pub async fn create_bot_handler(
    State(state): State<SharedState>,
    Json(payload): Json<CreateBotPayload>,
) -> ApiResult {
    if payload.name.trim().is_empty() || payload.platform.trim().is_empty() {
        return Err(ApiError::bad_request("Missing name/platform"));
    }

    // Discord is in-process (no Docker container).
    if payload.platform.eq_ignore_ascii_case("discord") {
        let id = format!("discord_{}", now_unix_secs()?);
        let bot = BotInstance {
            id: id.clone(),
            name: payload.name,
            platform: "Discord".to_string(),
            is_connected: false,
            is_running: false,
            container_id: None,
            ws_host: None,
            ws_port: None,
            webui_host: None,
            webui_port: None,
            webui_token: None,
            qq_id: None,
            linked_database: None,
            metadata: serde_json::json!({ "discord": { "token": "" } }),
            modules_config: HashMap::new(),
        };

        state.bots.insert(id.clone(), bot);
        save_bots(&state.bots);
        info!("已创建新机器人实例: {} (Discord)", id);

        return Ok(Json(serde_json::json!({
            "status": "success",
            "id": id,
        })));
    }

    // Default: QQ (NapCat OneBot via Docker). This may involve pulling a large image,
    // so we run provisioning in background and expose progress via /api/tasks.
    let bot_id = format!("{}_{}", payload.platform.to_lowercase(), now_unix_secs()?);
    let task_id = format!("create_bot_{}_{}", bot_id, now_unix_secs_lossy());

    let now = now_unix_secs_lossy();
    state.tasks.insert(
        task_id.clone(),
        BackgroundTask {
            id: task_id.clone(),
            kind: "create_bot".to_string(),
            title: format!("创建实例：{}", payload.name),
            state: TaskState::Running,
            progress: Some(TaskProgress {
                current: 0,
                total: 3,
                label: "准备中".to_string(),
            }),
            detail: Some("任务已创建，开始准备...".to_string()),
            result: None,
            error: None,
            created_at: now,
            updated_at: now,
        },
    );

    let state_bg = state.clone();
    let name = payload.name.clone();
    let platform = payload.platform.clone();
    let task_id_bg = task_id.clone();
    let bot_id_bg = bot_id.clone();

    tokio::spawn(async move {
        let total = 3u32;
        let napcat_image = std::env::var("NBOT_NAPCAT_IMAGE")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "docker.nailed.dev/mlikiowa/napcat-docker:latest".to_string());

        task_progress(&state_bg, &task_id_bg, 1, total, "检查镜像");
        if let Err(e) = ensure_docker_image(&state_bg, &task_id_bg, &napcat_image).await {
            task_fail(&state_bg, &task_id_bg, e.to_string());
            return;
        }

        task_progress(&state_bg, &task_id_bg, 2, total, "创建容器");
        task_detail(&state_bg, &task_id_bg, "正在创建 NapCat 容器与配置...");
        let provisioned = match provision_napcat_bot_container(&bot_id_bg).await {
            Ok(p) => p,
            Err(e) => {
                task_fail(&state_bg, &task_id_bg, e.to_string());
                return;
            }
        };

        task_progress(&state_bg, &task_id_bg, 3, total, "写入状态");
        let ws_port = provisioned.ws_port;
        let webui_port = provisioned.webui_port;
        let bot = build_running_bot_instance(
            bot_id_bg.clone(),
            name,
            platform,
            provisioned,
            None,
            serde_json::json!({}),
            HashMap::new(),
        );

        state_bg.bots.insert(bot_id_bg.clone(), bot);
        save_bots(&state_bg.bots);
        info!(
            "已创建新机器人实例: {}，WS端口 {}, WebUI端口 {}",
            bot_id_bg, ws_port, webui_port
        );

        task_detail(&state_bg, &task_id_bg, "创建完成");
        task_success(
            &state_bg,
            &task_id_bg,
            serde_json::json!({ "id": bot_id_bg, "ws_port": ws_port, "webui_port": webui_port }),
        );
    });

    Ok(Json(serde_json::json!({
        "status": "accepted",
        "id": bot_id,
        "task_id": task_id,
    })))
}

pub async fn delete_bot_handler(
    State(state): State<SharedState>,
    Extension(runtime): Extension<std::sync::Arc<crate::bot::BotRuntime>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    info!("删除机器人实例: {}", id);

    let bot = if let Some(bot) = state.bots.get(&id) {
        bot.clone()
    } else {
        return Json(serde_json::json!({ "status": "error", "message": "Bot not found" }));
    };

    if bot.platform.eq_ignore_ascii_case("discord") {
        runtime.shutdown_discord_connection(&id).await;
    } else {
        let container_id = bot.container_id.clone().unwrap_or(id.clone());
        let _ = Command::new("docker")
            .args(["stop", &container_id])
            .output()
            .await;
        let _ = Command::new("docker")
            .args(["rm", &container_id])
            .output()
            .await;

        let docker_mode = std::env::var("NBOT_DOCKER_MODE")
            .ok()
            .map(|v| {
                let v = v.trim();
                v.eq_ignore_ascii_case("1")
                    || v.eq_ignore_ascii_case("true")
                    || v.eq_ignore_ascii_case("yes")
            })
            .unwrap_or(false);

        if docker_mode {
            let alias = id.replace('_', "-");
            let config_volume = format!("nbot-bot-{}-config", alias);
            let qq_volume = format!("nbot-bot-{}-qq", alias);
            let _ = Command::new("docker")
                .args(["volume", "rm", &config_volume])
                .output()
                .await;
            let _ = Command::new("docker")
                .args(["volume", "rm", &qq_volume])
                .output()
                .await;
        } else if let Ok((config_path, _qq_path)) = bot_dirs(&id) {
            if let Some(base_dir) = config_path.parent() {
                let _ = std::fs::remove_dir_all(base_dir);
            }
        }
    }

    state.bots.remove(&id);
    save_bots(&state.bots);

    info!("机器人 {} 删除成功", id);
    Json(serde_json::json!({ "status": "success" }))
}

#[derive(serde::Deserialize)]
pub struct UpdateBotPayload {
    pub name: Option<String>,
    pub linked_database: Option<String>,
}

pub async fn update_bot_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateBotPayload>,
) -> Json<serde_json::Value> {
    if let Some(mut bot) = state.bots.get_mut(&id) {
        if let Some(name) = payload.name {
            if !name.is_empty() {
                bot.name = name;
            }
        }
        if let Some(linked_db) = payload.linked_database {
            bot.linked_database = if linked_db.is_empty() {
                None
            } else {
                Some(linked_db)
            };
        }
        drop(bot);
        save_bots(&state.bots);
        info!("已更新机器人 {}", id);
        Json(serde_json::json!({ "status": "success" }))
    } else {
        Json(serde_json::json!({ "status": "error", "message": "Bot not found" }))
    }
}

#[derive(serde::Deserialize)]
pub struct UpdateDiscordBotPayload {
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub is_running: Option<bool>,
}

pub async fn update_discord_bot_handler(
    State(state): State<SharedState>,
    Extension(runtime): Extension<std::sync::Arc<crate::bot::BotRuntime>>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateDiscordBotPayload>,
) -> Json<serde_json::Value> {
    let Some(mut bot) = state.bots.get_mut(&id) else {
        return Json(serde_json::json!({ "status": "error", "message": "Bot not found" }));
    };

    if !bot.platform.eq_ignore_ascii_case("discord") {
        return Json(serde_json::json!({ "status": "error", "message": "Not a Discord bot" }));
    }

    let mut need_restart = false;
    if let Some(token) = payload.token {
        let token = token.trim().to_string();
        if token.is_empty() {
            return Json(serde_json::json!({ "status": "error", "message": "Missing token" }));
        }
        bot.metadata["discord"]["token"] = serde_json::json!(token);
        need_restart = true;
    }

    if let Some(running) = payload.is_running {
        if bot.is_running != running {
            bot.is_running = running;
            need_restart = true;
        }
    }

    drop(bot);
    save_bots(&state.bots);

    if need_restart {
        runtime.shutdown_discord_connection(&id).await;
    }

    Json(serde_json::json!({ "status": "success" }))
}

pub async fn list_bots_for_link_handler(
    State(state): State<SharedState>,
) -> Json<Vec<serde_json::Value>> {
    let bots: Vec<serde_json::Value> = state
        .bots
        .iter()
        .map(|b| {
            serde_json::json!({ "id": b.id, "name": b.name, "linked_database": b.linked_database })
        })
        .collect();
    Json(bots)
}

pub async fn get_bot_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    if let Some(bot) = state.bots.get(&id) {
        Json(serde_json::json!({ "status": "success", "bot": sanitize_bot_for_api(bot.clone()) }))
    } else {
        Json(serde_json::json!({ "status": "error", "message": "Bot not found" }))
    }
}

#[derive(serde::Deserialize)]
pub struct CopyBotPayload {
    pub new_name: String,
}

pub async fn copy_bot_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(payload): Json<CopyBotPayload>,
) -> ApiResult {
    let source_bot = if let Some(bot) = state.bots.get(&id) {
        bot.clone()
    } else {
        return Err(ApiError::not_found("Source bot not found"));
    };

    // Check name uniqueness
    for bot in state.bots.iter() {
        if bot.name == payload.new_name {
            return Err(ApiError::bad_request("Bot name already exists"));
        }
    }

    if source_bot.platform.eq_ignore_ascii_case("discord") {
        let new_id = format!("discord_{}", now_unix_secs()?);
        let mut metadata = source_bot.metadata;
        if let Some(obj) = metadata.get_mut("discord") {
            if let Some(map) = obj.as_object_mut() {
                map.remove("token");
                map.remove("bot_user_id");
            }
        }

        let new_bot = BotInstance {
            id: new_id.clone(),
            name: payload.new_name,
            platform: "Discord".to_string(),
            is_connected: false,
            is_running: false,
            container_id: None,
            ws_host: None,
            ws_port: None,
            webui_host: None,
            webui_port: None,
            webui_token: None,
            qq_id: None,
            linked_database: source_bot.linked_database,
            metadata,
            modules_config: source_bot.modules_config,
        };

        state.bots.insert(new_id.clone(), new_bot);
        save_bots(&state.bots);
        info!("复制机器人成功: {} -> {}", id, new_id);

        return Ok(Json(
            serde_json::json!({ "status": "success", "id": new_id }),
        ));
    }

    let new_id = format!(
        "{}_{}",
        source_bot.platform.to_lowercase(),
        now_unix_secs()?
    );
    let provisioned = provision_napcat_bot_container(&new_id).await?;

    let new_bot = build_running_bot_instance(
        new_id.clone(),
        payload.new_name,
        source_bot.platform,
        provisioned,
        source_bot.linked_database,
        source_bot.metadata,
        source_bot.modules_config,
    );

    state.bots.insert(new_id.clone(), new_bot);
    save_bots(&state.bots);
    info!("复制机器人成功: {} -> {}", id, new_id);

    Ok(Json(
        serde_json::json!({ "status": "success", "id": new_id }),
    ))
}
