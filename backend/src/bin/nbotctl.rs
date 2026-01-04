use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn die(msg: &str) -> ! {
    eprintln!("ERROR: {msg}");
    std::process::exit(1);
}

fn read_env_file(path: &Path) -> HashMap<String, String> {
    let mut out = HashMap::<String, String>::new();
    let Ok(content) = std::fs::read_to_string(path) else {
        return out;
    };
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let v = v.trim();
        if !k.is_empty() && !v.is_empty() {
            out.insert(k.to_string(), v.to_string());
        }
    }
    out
}

fn resolve_install_dir(arg: Option<String>) -> PathBuf {
    if let Some(dir) = arg {
        return PathBuf::from(dir);
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn resolve_token(arg: Option<String>, install_dir: &Path) -> String {
    if let Some(t) = arg {
        let t = t.trim().to_string();
        if !t.is_empty() {
            return t;
        }
    }

    if let Ok(t) = std::env::var("NBOT_API_TOKEN") {
        let t = t.trim().to_string();
        if !t.is_empty() {
            return t;
        }
    }

    let env_file = install_dir.join(".env");
    if env_file.exists() {
        let cfg = read_env_file(&env_file);
        if let Some(t) = cfg.get("NBOT_API_TOKEN").map(|s| s.trim().to_string()) {
            if !t.is_empty() {
                return t;
            }
        }
    }

    let token_file = install_dir.join("data").join("state").join("api_token.txt");
    if token_file.exists() {
        if let Ok(t) = std::fs::read_to_string(&token_file) {
            let t = t.lines().next().unwrap_or("").trim().to_string();
            if !t.is_empty() {
                return t;
            }
        }
    }

    die("缺少 API Token：请使用 --token 或设置 NBOT_API_TOKEN，或在安装目录中提供 data/state/api_token.txt");
}

fn resolve_base_url(arg: Option<String>, install_dir: &Path) -> String {
    if let Some(url) = arg {
        let url = url.trim().to_string();
        if !url.is_empty() {
            return url.trim_end_matches('/').to_string();
        }
    }

    if let Ok(url) = std::env::var("NBOT_URL") {
        let url = url.trim().to_string();
        if !url.is_empty() {
            return url.trim_end_matches('/').to_string();
        }
    }

    let env_file = install_dir.join(".env");
    let mut port: Option<u16> = None;
    if env_file.exists() {
        let cfg = read_env_file(&env_file);
        port = cfg
            .get("NBOT_WEBUI_PORT")
            .and_then(|s| s.trim().parse::<u16>().ok())
            .or_else(|| {
                cfg.get("NBOT_PORT")
                    .and_then(|s| s.trim().parse::<u16>().ok())
            });
    }

    let port = port.unwrap_or(32100);
    format!("http://127.0.0.1:{port}")
}

fn is_local_base_url(url: &str) -> bool {
    let url = url.trim().to_ascii_lowercase();
    url.starts_with("http://127.0.0.1")
        || url.starts_with("https://127.0.0.1")
        || url.starts_with("http://localhost")
        || url.starts_with("https://localhost")
        || url.starts_with("http://[::1]")
        || url.starts_with("https://[::1]")
}

fn build_client(token: &str, base_url: &str) -> reqwest::Client {
    let mut headers = HeaderMap::new();
    let value = HeaderValue::from_str(&format!("Bearer {token}"))
        .unwrap_or_else(|_| HeaderValue::from_static("Bearer"));
    headers.insert(AUTHORIZATION, value);

    let mut builder = reqwest::Client::builder();
    if is_local_base_url(base_url) {
        // On some environments (Windows proxy), reqwest may route localhost via system proxy.
        // For local debugging, always bypass proxies to avoid 502/connection issues.
        builder = builder.no_proxy();
    }

    builder
        .default_headers(headers)
        .timeout(Duration::from_secs(15))
        .build()
        .expect("failed to build reqwest client")
}

async fn http_json(client: &reqwest::Client, method: &str, url: &str) -> Result<Value, String> {
    let req = match method {
        "GET" => client.get(url),
        "POST" => client.post(url),
        _ => return Err(format!("unsupported method: {method}")),
    };
    let resp = req
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if status == 401 || status == 403 {
        return Err("unauthorized (token invalid)".to_string());
    }
    if !status.is_success() {
        return Err(format!("HTTP {status}: {text}"));
    }
    serde_json::from_str(&text).map_err(|e| format!("invalid json: {e}: {text}"))
}

fn print_json(v: &Value) {
    match serde_json::to_string_pretty(v) {
        Ok(s) => println!("{s}"),
        Err(_) => println!("{v}"),
    }
}

fn as_str(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

fn as_bool(v: &Value, key: &str) -> bool {
    v.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

fn as_u16_vec(v: &Value, key: &str) -> Vec<u16> {
    v.get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_u64().and_then(|n| u16::try_from(n).ok()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn usage() -> ! {
    eprintln!(
        r#"nbotctl - nBot CLI

Usage:
  nbotctl [--dir <install_dir>] [--url <base_url>] [--token <token>] <command> [args...]

Config resolution (if omitted):
  - token: --token > NBOT_API_TOKEN env > <dir>/.env (NBOT_API_TOKEN) > <dir>/data/state/api_token.txt
  - url:   --url > NBOT_URL env > http://127.0.0.1:<NBOT_WEBUI_PORT|NBOT_PORT|32100>

Commands:
  status                 List bot instances (/api/status)
  tools                  List tools (/api/tools)
  infra                  List infra tools only (/api/tools, kind=infra)
  tools <action> <id>    Action: start|stop|restart|recreate|pull
  tasks                  List background tasks (/api/tasks)
  qr                     Show latest QR (/api/napcat/qr)
  get <path>             GET /api/<path> (raw JSON)
"#
    );
    std::process::exit(2);
}

#[tokio::main]
async fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    let _exe = args.remove(0);

    let mut install_dir: Option<String> = None;
    let mut base_url: Option<String> = None;
    let mut token: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => usage(),
            "--dir" => {
                i += 1;
                install_dir = args.get(i).cloned();
            }
            "--url" => {
                i += 1;
                base_url = args.get(i).cloned();
            }
            "--token" => {
                i += 1;
                token = args.get(i).cloned();
            }
            _ => break,
        }
        i += 1;
    }

    let args = args[i..].to_vec();
    if args.is_empty() {
        usage();
    }

    let install_dir = resolve_install_dir(install_dir);
    let token = resolve_token(token, &install_dir);
    let base_url = resolve_base_url(base_url, &install_dir);
    let api = format!("{}/api", base_url.trim_end_matches('/'));
    let client = build_client(&token, &base_url);

    match args[0].as_str() {
        "status" => {
            let v = http_json(&client, "GET", &format!("{api}/status"))
                .await
                .unwrap_or_else(|e| die(&e));
            if let Some(arr) = v.as_array() {
                println!("bots: {}", arr.len());
                for b in arr {
                    let id = as_str(b, "id");
                    let name = as_str(b, "name");
                    let platform = as_str(b, "platform");
                    let running = as_bool(b, "is_running");
                    let connected = as_bool(b, "is_connected");
                    println!(
                        "- {id} | {platform} | running={} connected={} | {name}",
                        if running { "yes" } else { "no" },
                        if connected { "yes" } else { "no" }
                    );
                }
            } else {
                print_json(&v);
            }
        }
        "tools" | "infra" => {
            let v = http_json(&client, "GET", &format!("{api}/tools"))
                .await
                .unwrap_or_else(|e| die(&e));
            let want_kind = if args[0].as_str() == "infra" {
                Some("infra")
            } else {
                None
            };

            if args.len() == 3 && args[0].as_str() == "tools" {
                let action = args[1].as_str();
                let id = args[2].as_str();
                let allowed = matches!(action, "start" | "stop" | "restart" | "recreate" | "pull");
                if !allowed {
                    die("unknown tools action (use: start|stop|restart|recreate|pull)");
                }
                let out = http_json(&client, "POST", &format!("{api}/tools/{id}/{action}"))
                    .await
                    .unwrap_or_else(|e| die(&e));
                print_json(&out);
                return;
            }

            let Some(arr) = v.as_array() else {
                print_json(&v);
                return;
            };

            let list = arr.iter().filter(|t| {
                if let Some(kind) = want_kind {
                    t.get("kind").and_then(|v| v.as_str()) == Some(kind)
                } else {
                    true
                }
            });

            for t in list {
                let id = as_str(t, "id");
                let kind = as_str(t, "kind");
                let status = as_str(t, "status");
                let name = as_str(t, "name");
                let ports = as_u16_vec(t, "ports");
                let detail = t
                    .get("detail")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                let ports = if ports.is_empty() {
                    String::new()
                } else {
                    ports
                        .iter()
                        .map(|p| p.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                };
                if detail.is_empty() {
                    println!("- {id} | {kind} | {status} | ports=[{ports}] | {name}");
                } else {
                    println!("- {id} | {kind} | {status} | ports=[{ports}] | {name} | {detail}");
                }
            }
        }
        "tasks" => {
            let v = http_json(&client, "GET", &format!("{api}/tasks"))
                .await
                .unwrap_or_else(|e| die(&e));
            print_json(&v);
        }
        "qr" => {
            let v = http_json(&client, "GET", &format!("{api}/napcat/qr"))
                .await
                .unwrap_or_else(|e| die(&e));
            print_json(&v);
        }
        "get" => {
            if args.len() != 2 {
                die("usage: nbotctl get <path> (example: nbotctl get tools)");
            }
            let path = args[1].trim().trim_start_matches('/');
            let v = http_json(&client, "GET", &format!("{api}/{path}"))
                .await
                .unwrap_or_else(|e| die(&e));
            print_json(&v);
        }
        _ => usage(),
    }
}
