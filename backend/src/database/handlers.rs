use crate::models::{DatabaseInstance, SharedState};
use crate::persistence::{save_bots, save_databases};
use axum::extract::{Json, State};
use tokio::process::Command;
use tracing::{error, info};

#[derive(serde::Deserialize)]
pub struct CreateDatabasePayload {
    pub name: String,
    pub db_type: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

fn generate_password() -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnpqrstuvwxyz23456789";
    let mut rng = rand::rng();
    (0..16)
        .map(|_| CHARSET[rng.random_range(0..CHARSET.len())] as char)
        .collect()
}

pub async fn list_databases_handler(
    State(state): State<SharedState>,
) -> Json<Vec<DatabaseInstance>> {
    let dbs: Vec<DatabaseInstance> = state
        .databases
        .iter()
        .map(|kv| kv.value().clone())
        .collect();
    Json(dbs)
}

pub async fn create_database_handler(
    State(state): State<SharedState>,
    Json(payload): Json<CreateDatabasePayload>,
) -> Json<serde_json::Value> {
    let now_secs = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs(),
        Err(e) => {
            return Json(
                serde_json::json!({ "status": "error", "message": format!("System time error: {}", e) }),
            );
        }
    };
    let id = format!("db_{}_{}", payload.db_type, now_secs);

    let mut host_port: u16 = 15432;
    for db in state.databases.iter() {
        if db.host_port >= host_port {
            host_port = db.host_port + 1;
        }
    }

    let username = payload.username.unwrap_or_else(|| "admin".to_string());
    let password = payload.password.unwrap_or_else(generate_password);
    let database_name = payload.name.clone().replace(" ", "_").to_lowercase();

    let (image, internal_port, env_args): (&str, u16, Vec<String>) = match payload.db_type.as_str()
    {
        "postgres" => (
            "postgres:16-alpine",
            5432,
            vec![
                "-e".to_string(),
                format!("POSTGRES_USER={}", username),
                "-e".to_string(),
                format!("POSTGRES_PASSWORD={}", password),
                "-e".to_string(),
                format!("POSTGRES_DB={}", database_name),
            ],
        ),
        "mysql" => (
            "mysql:8",
            3306,
            vec![
                "-e".to_string(),
                format!("MYSQL_ROOT_PASSWORD={}", password),
                "-e".to_string(),
                format!("MYSQL_USER={}", username),
                "-e".to_string(),
                format!("MYSQL_PASSWORD={}", password),
                "-e".to_string(),
                format!("MYSQL_DATABASE={}", database_name),
            ],
        ),
        "redis" => ("redis:alpine", 6379, vec![]),
        _ => {
            return Json(
                serde_json::json!({ "status": "error", "message": "Unsupported database type" }),
            );
        }
    };

    let _ = Command::new("docker")
        .args(["network", "create", "nbot_default"])
        .output()
        .await;

    info!(
        "创建 {} 数据库容器 {}，端口 {}",
        payload.db_type, id, host_port
    );

    let port_mapping = format!("{}:{}", host_port, internal_port);
    let mut args = vec![
        "run".to_string(),
        "-d".to_string(),
        "--name".to_string(),
        id.clone(),
        "--restart".to_string(),
        "always".to_string(),
        "--network".to_string(),
        "nbot_default".to_string(),
        "-p".to_string(),
        port_mapping,
        "--label".to_string(),
        "com.docker.compose.project=nbot".to_string(),
        "--label".to_string(),
        format!("com.docker.compose.service={}", payload.db_type),
    ];
    args.extend(env_args);
    args.push(image.to_string());

    let output = Command::new("docker").args(&args).output().await;

    match output {
        Ok(out) if !out.status.success() => {
            let err = String::from_utf8_lossy(&out.stderr);
            error!("数据库容器创建失败: {}", err);
            return Json(
                serde_json::json!({ "status": "error", "message": format!("Docker 错误: {}", err) }),
            );
        }
        Err(e) => {
            error!("执行 docker 失败: {}", e);
            return Json(serde_json::json!({ "status": "error", "message": e.to_string() }));
        }
        _ => {
            info!("数据库容器 {} 创建成功", id);
        }
    }

    let db = DatabaseInstance {
        id: id.clone(),
        name: payload.name,
        db_type: payload.db_type,
        container_id: Some(id.clone()),
        host_port,
        internal_port,
        username: username.clone(),
        password: password.clone(),
        database_name: database_name.clone(),
        is_running: true,
    };

    state.databases.insert(id.clone(), db);
    save_databases(&state.databases);

    Json(serde_json::json!({
        "status": "success",
        "id": id,
        "host_port": host_port,
        "username": username,
        "password": password,
        "database": database_name
    }))
}

pub async fn delete_database_handler(
    State(state): State<SharedState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Json<serde_json::Value> {
    info!("删除数据库: {}", id);

    if let Some((_, db)) = state.databases.remove(&id) {
        if let Some(container_id) = db.container_id {
            let _ = Command::new("docker")
                .args(["stop", &container_id])
                .output()
                .await;
            let _ = Command::new("docker")
                .args(["rm", &container_id])
                .output()
                .await;
        }
        save_databases(&state.databases);
        Json(serde_json::json!({ "status": "success" }))
    } else {
        Json(serde_json::json!({ "status": "error", "message": "Database not found" }))
    }
}

#[derive(serde::Deserialize)]
pub struct LinkDatabasePayload {
    pub bot_id: String,
    pub database_id: Option<String>,
}

pub async fn link_database_handler(
    State(state): State<SharedState>,
    Json(payload): Json<LinkDatabasePayload>,
) -> Json<serde_json::Value> {
    if let Some(ref db_id) = payload.database_id {
        if !state.databases.contains_key(db_id) {
            return Json(serde_json::json!({ "status": "error", "message": "Database not found" }));
        }
    }

    if let Some(mut bot) = state.bots.get_mut(&payload.bot_id) {
        bot.linked_database = payload.database_id.clone();
        drop(bot);
        save_bots(&state.bots);
        if let Some(db_id) = payload.database_id {
            info!("已关联数据库 {} 到机器人 {}", db_id, payload.bot_id);
        } else {
            info!("已解除机器人 {} 的数据库关联", payload.bot_id);
        }
        Json(serde_json::json!({ "status": "success" }))
    } else {
        Json(serde_json::json!({ "status": "error", "message": "Bot not found" }))
    }
}
