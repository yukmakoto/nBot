mod auth;
mod bot;
mod command;
mod container;
mod database;
mod http;
mod logs;
mod models;
mod module;
mod persistence;
mod plugin;
mod plugin_handlers;
pub mod qq_face;
mod render_image;
mod task;
mod tool;
pub mod utils;

use axum::http::HeaderValue;
use axum::{
    middleware,
    routing::{delete, get, post, put},
    Extension, Router,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::auth::{load_or_create_api_token, require_api_token, AuthState};
use crate::bot::{
    docker_status_sync_loop, napcat_login_monitor, start_bot_connections,
    start_discord_connections, BotRuntime,
};
use crate::command::CommandRegistry;
use crate::models::{AppState, BotInstance, MessageStats, RuntimeState};
use crate::module::ModuleRegistry;
use crate::persistence::{load_bots, load_databases};
use crate::plugin::{PluginManager, PluginRegistry};

#[tokio::main]
async fn main() {
    let logs = Arc::new(logs::LogStore::new(5000));
    let writer = logs::TeeMakeWriter::new(logs.clone());
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(writer).with_ansi(false))
        .init();
    info!("启动 nBot 后端...");

    // Load persisted state
    let bots = load_bots();
    let databases = load_databases();

    // Reset is_connected on startup - let napcat_login_monitor detect actual state
    for mut bot in bots.iter_mut() {
        bot.is_connected = false;
    }

    // Migration: remove legacy infrastructure bot (NapCat is per-QQ-instance, not a global infra).
    if bots.remove("napcat_core").is_some() {
        crate::persistence::save_bots(&bots);
    }

    // Initialize plugin registry
    let data_dir = std::env::var("NBOT_DATA_DIR").unwrap_or_else(|_| "data".to_string());
    let plugins = Arc::new(PluginRegistry::new(&data_dir));
    info!("插件注册表已初始化，共 {} 个插件", plugins.list().len());

    // Initialize plugin manager
    let plugin_manager = Arc::new(PluginManager::new(&data_dir));

    // Initialize module registry
    let modules = Arc::new(ModuleRegistry::new(&data_dir));
    info!("模块注册表已初始化，共 {} 个模块", modules.list().len());

    // Initialize command registry
    let commands = Arc::new(CommandRegistry::new(&data_dir));
    info!("指令注册表已初始化，共 {} 个指令", commands.list().len());

    // Initialize message stats
    let message_stats = Arc::new(MessageStats::new());

    let api_token = load_or_create_api_token(&data_dir);
    let auth_state = Arc::new(AuthState { api_token });

    let state = Arc::new(AppState {
        bots,
        databases,
        tasks: dashmap::DashMap::new(),
        runtime: Arc::new(RuntimeState {
            latest_qr: RwLock::new(None),
            latest_qr_image: RwLock::new(None),
        }),
        logs: logs.clone(),
        plugins: plugins.clone(),
        plugin_manager: plugin_manager.clone(),
        modules,
        commands: commands.clone(),
        message_stats,
    });

    // If configured, bootstrap official plugins from the market (first-run only).
    // This enables "no bundled plugins" deployments where nbot-site is the source of truth.
    plugin_handlers::bootstrap_official_plugins_startup(&state).await;

    // Load enabled plugins
    for plugin in plugins.list_enabled() {
        if plugin_manager.is_loaded(&plugin.manifest.id) {
            continue;
        }
        if let Err(e) = plugin_manager.load(&plugin).await {
            error!("加载插件 {} 失败: {}", plugin.manifest.id, e);
        } else {
            plugin_handlers::register_plugin_commands(&commands, &plugin);
        }
    }

    // Startup: Scan existing Docker containers and rebuild Bot list
    info!("扫描现有 Docker 容器中的 QQ 机器人...");
    let docker_mode = std::env::var("NBOT_DOCKER_MODE")
        .ok()
        .map(|v| {
            let v = v.trim();
            v.eq_ignore_ascii_case("1")
                || v.eq_ignore_ascii_case("true")
                || v.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false);
    if let Ok(output) = tokio::process::Command::new("docker")
        .args([
            "ps",
            "-a",
            "--filter",
            "name=qq_",
            "--format",
            "{{.Names}}|{{.Ports}}|{{.State}}",
        ])
        .output()
        .await
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 3 {
                let name = parts[0].to_string();
                let ports = parts[1];
                let container_state = parts[2];

                let ws_port: Option<u16> = ports
                    .split(',')
                    .find(|p| p.contains("3001"))
                    .and_then(|p| p.split(':').nth(1))
                    .and_then(|s| s.split("->").next())
                    .and_then(|s| s.parse().ok());

                let webui_port: Option<u16> = ports
                    .split(',')
                    .find(|p| p.contains("6099"))
                    .and_then(|p| p.split(':').nth(1))
                    .and_then(|s| s.split("->").next())
                    .and_then(|s| s.parse().ok());

                let (ws_host, ws_port) = if docker_mode {
                    (Some(name.replace('_', "-")), Some(3001))
                } else {
                    (Some("127.0.0.1".to_string()), ws_port)
                };
                let (webui_host, webui_port) = if docker_mode {
                    (Some(name.replace('_', "-")), Some(6099))
                } else {
                    (Some("127.0.0.1".to_string()), webui_port)
                };

                if ws_port.is_some() {
                    info!(
                        "发现现有机器人容器: {} (WS端口: {:?}, WebUI端口: {:?}, 状态: {})",
                        name, ws_port, webui_port, container_state
                    );
                    // Only update running status, don't overwrite existing bot data
                    if let Some(mut existing) = state.bots.get_mut(&name) {
                        existing.is_running = container_state == "running";
                        existing.ws_host = ws_host.clone();
                        existing.ws_port = ws_port;
                        existing.webui_host = webui_host.clone();
                        existing.webui_port = webui_port;
                    } else {
                        // Only create new bot if it doesn't exist
                        state.bots.insert(
                            name.clone(),
                            BotInstance {
                                id: name.clone(),
                                name: name.replace("qq_", "QQ Bot "),
                                platform: "QQ".to_string(),
                                is_connected: false,
                                is_running: container_state == "running",
                                container_id: Some(name.clone()),
                                ws_host,
                                ws_port,
                                webui_host,
                                webui_port,
                                webui_token: None,
                                qq_id: None,
                                linked_database: None,
                                metadata: serde_json::json!({}),
                                modules_config: std::collections::HashMap::new(),
                            },
                        );
                    }
                }
            }
        }
    }
    info!("共发现 {} 个机器人", state.bots.len());

    // Start Loops
    let state_cl1 = state.clone();
    tokio::spawn(async move {
        napcat_login_monitor(state_cl1).await;
    });

    let state_cl2 = state.clone();
    tokio::spawn(async move {
        docker_status_sync_loop(state_cl2).await;
    });

    // Start bot message listener
    let bot_runtime = Arc::new(BotRuntime::new());
    let state_cl4 = state.clone();
    let runtime_cl = bot_runtime.clone();
    tokio::spawn(async move {
        start_bot_connections(state_cl4, runtime_cl).await;
    });

    // Start Discord connection manager (in-process bots)
    let state_cl5 = state.clone();
    let runtime_cl5 = bot_runtime.clone();
    tokio::spawn(async move {
        start_discord_connections(state_cl5, runtime_cl5).await;
    });

    let allowed_origins = std::env::var("NBOT_ALLOWED_ORIGINS")
        .ok()
        .and_then(|v| {
            let origins: Vec<HeaderValue> = v
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .filter_map(|s| HeaderValue::from_str(s).ok())
                .collect();
            if origins.is_empty() {
                None
            } else {
                Some(origins)
            }
        })
        .unwrap_or_else(|| {
            vec![
                HeaderValue::from_static("http://localhost:32100"),
                HeaderValue::from_static("http://127.0.0.1:32100"),
                HeaderValue::from_static("http://localhost:3000"),
                HeaderValue::from_static("http://127.0.0.1:3000"),
            ]
        });

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(allowed_origins))
        .allow_methods(AllowMethods::any())
        .allow_headers(AllowHeaders::any());

    let api = Router::new()
        // Bot routes
        .route("/status", get(bot::get_status_handler))
        .route("/system/stats", get(bot::get_system_stats_handler))
        .route("/system/info", get(bot::get_system_info_handler))
        .route("/system/logs", get(bot::get_system_logs_handler))
        .route("/system/export", get(bot::system_export_handler))
        .route("/docker/info", get(bot::get_docker_info_handler))
        .route("/message/stats", get(bot::get_message_stats_handler))
        .route("/bots", post(bot::create_bot_handler))
        .route("/bots/list", get(bot::list_bots_for_link_handler))
        .route("/bots/:id", get(bot::get_bot_handler))
        .route("/bots/:id", delete(bot::delete_bot_handler))
        .route("/bots/:id", put(bot::update_bot_handler))
        .route("/bots/:id/discord", put(bot::update_discord_bot_handler))
        .route("/bots/:id/login", post(bot::login_trigger_handler))
        .route("/bots/:id/copy", post(bot::copy_bot_handler))
        .route(
            "/bots/:id/modules",
            get(bot::list_bot_effective_modules_handler),
        )
        .route("/bots/:id/module", put(bot::update_bot_module_handler))
        .route(
            "/bots/:id/module/:module_id",
            get(bot::get_bot_effective_module_handler)
                .delete(bot::delete_bot_module_override_handler),
        )
        .route("/napcat/qr", get(bot::qr_handler).delete(bot::qr_clear_handler))
        // Task routes
        .route("/tasks", get(task::list_tasks_handler))
        .route("/tasks/:id", delete(task::delete_task_handler))
        // Container routes
        .route("/docker/list", get(container::list_containers_handler))
        .route("/docker/action", post(container::container_action_handler))
        .route("/docker/logs", get(container::container_logs_handler))
        // Database routes
        .route("/databases", get(database::list_databases_handler))
        .route("/databases", post(database::create_database_handler))
        .route("/databases/:id", delete(database::delete_database_handler))
        .route("/bots/link-database", post(database::link_database_handler))
        // Plugin routes
        .route(
            "/plugins/installed",
            get(plugin_handlers::list_installed_handler),
        )
        .route(
            "/plugins/install",
            post(plugin_handlers::install_plugin_handler),
        )
        .route(
            "/plugins/package",
            post(plugin_handlers::install_package_handler),
        )
        .route("/plugins/sign", post(plugin_handlers::sign_plugin_handler))
        .route(
            "/plugins/:id",
            delete(plugin_handlers::uninstall_plugin_handler),
        )
        .route(
            "/plugins/:id/enable",
            post(plugin_handlers::enable_plugin_handler),
        )
        .route(
            "/plugins/:id/disable",
            post(plugin_handlers::disable_plugin_handler),
        )
        .route(
            "/plugins/:id/config",
            post(plugin_handlers::update_plugin_config_handler),
        )
        // Market routes
        .route(
            "/market/plugins",
            get(plugin_handlers::list_market_plugins_handler),
        )
        .route(
            "/market/install",
            post(plugin_handlers::install_from_market_handler),
        )
        .route(
            "/market/sync",
            post(plugin_handlers::sync_official_plugins_handler),
        )
        // Module routes
        .route("/modules", get(module::list_modules_handler))
        .route("/modules/:id", get(module::get_module_handler))
        .route("/modules/:id/enable", post(module::enable_module_handler))
        .route("/modules/:id/disable", post(module::disable_module_handler))
        .route(
            "/modules/:id/config",
            put(module::update_module_config_handler),
        )
        // LLM routes
        .route("/llm/config", get(module::get_llm_config_handler))
        .route("/llm/config", put(module::update_llm_config_handler))
        .route("/llm/test", post(module::llm_test_handler))
        .route("/llm/models", post(module::llm_models_handler))
        .route("/llm/chat", post(module::llm_chat_handler))
        .route("/llm/tavily/test", post(module::tavily_test_handler))
        // Command routes
        .route("/commands", get(command::list_commands_handler))
        .route("/commands", post(command::create_command_handler))
        .route("/commands/:id", get(command::get_command_handler))
        .route("/commands/:id", put(command::update_command_handler))
        .route("/commands/:id", delete(command::delete_command_handler))
        // Tool routes
        .route("/tools", get(tool::list_tools_handler))
        .route("/tools/:id/start", post(tool::start_tool_handler))
        .route("/tools/:id/stop", post(tool::stop_tool_handler))
        .route("/tools/:id/restart", post(tool::restart_tool_handler))
        .route("/tools/:id/recreate", post(tool::recreate_tool_handler))
        .route("/tools/:id/pull", post(tool::pull_tool_handler))
        // Relations routes
        .route("/relations/friends", get(bot::get_friends_handler))
        .route("/relations/groups", get(bot::get_groups_handler))
        .route("/relations/group-members", get(bot::get_group_members_handler))
        .route("/relations/login-info", get(bot::get_login_info_handler))
        // Chat routes
        .route("/chat/history", get(bot::get_chat_history_handler))
        .route("/chat/send", post(bot::send_chat_message_handler))
        .layer(Extension(bot_runtime.clone()))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            auth_state.clone(),
            require_api_token,
        ));

    let app = Router::new()
        .nest("/api", api)
        .layer(cors)
        .fallback_service(tower_http::services::ServeDir::new("dist").precompressed_gzip())
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache, no-store, must-revalidate"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::PRAGMA,
            HeaderValue::from_static("no-cache"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::HeaderName::from_static("referrer-policy"),
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("camera=(), microphone=(), geolocation=(), payment=(), usb=()"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static(
                "default-src 'self'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'; \
                 connect-src 'self'; img-src 'self' data: https://q1.qlogo.cn https://p.qlogo.cn https://txz.qq.com https://im.qq.com https://api.qrserver.com https://cdn.jsdelivr.net; \
                 style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; \
                 script-src 'self' 'unsafe-eval' 'wasm-unsafe-eval'",
            ),
        ));

    let port: u16 = std::env::var("NBOT_PORT")
        .ok()
        .and_then(|s| s.trim().parse::<u16>().ok())
        .unwrap_or(32100);
    let bind = std::env::var("NBOT_BIND").unwrap_or_else(|_| format!("0.0.0.0:{}", port));

    let listener = match tokio::net::TcpListener::bind(&bind).await {
        Ok(l) => l,
        Err(e) => {
            error!("绑定端口失败 ({}): {}", bind, e);
            std::process::exit(1);
        }
    };
    let local = listener
        .local_addr()
        .map(|a| a.to_string())
        .unwrap_or(bind.clone());
    info!("服务器监听于 http://{}", local);
    if let Err(e) = axum::serve(listener, app).await {
        error!("HTTP 服务异常退出: {}", e);
    }
}
