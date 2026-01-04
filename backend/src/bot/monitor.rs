use crate::models::SharedState;
use crate::persistence::save_bots;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::sleep;
use tracing::{debug, error, info};

type BotsToCheckItem = (String, String, u16, Option<String>, Option<String>);

/// NapCat WebUI 登录响应
#[derive(Debug, Deserialize)]
struct AuthLoginResponse {
    code: i32,
    #[serde(default)]
    data: Option<AuthLoginData>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
struct AuthLoginData {
    Credential: String,
}

/// NapCat QQ登录状态响应
#[derive(Debug, Deserialize)]
struct QQLoginStatusResponse {
    code: i32,
    #[serde(default)]
    data: Option<QQLoginStatusData>,
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
struct QQLoginStatusData {
    #[serde(default)]
    isLogin: bool,
    #[serde(default)]
    #[serde(
        alias = "qrcodeUrl",
        alias = "qrcodeURL",
        alias = "qrcode_url",
        alias = "qrCodeUrl",
        alias = "qr_code_url"
    )]
    qrcodeurl: Option<String>,
}

/// 登录请求体
#[derive(Serialize)]
struct LoginRequest {
    hash: String,
}

/// 生成 NapCat WebUI 密码哈希
pub fn generate_password_hash(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{}.napcat", token));
    format!("{:x}", hasher.finalize())
}

async fn fetch_qr_image_data_url(client: &reqwest::Client, url: &str) -> Option<String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return None;
    }

    let resp = client
        .get(url)
        .header(reqwest::header::USER_AGENT, "nBot")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }

    let mime = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/png")
        .split(';')
        .next()
        .unwrap_or("image/png")
        .trim()
        .to_string();

    if !mime.starts_with("image/") {
        return None;
    }

    let bytes = resp.bytes().await.ok()?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some(format!("data:{mime};base64,{b64}"))
}

async fn webui_login(
    client: &reqwest::Client,
    login_url: &str,
    token: &str,
) -> Result<AuthLoginResponse, reqwest::Error> {
    let hash = generate_password_hash(token);
    let login_req = LoginRequest { hash };
    client
        .post(login_url)
        .json(&login_req)
        .send()
        .await?
        .json::<AuthLoginResponse>()
        .await
}

fn trim_token_like(value: &str) -> Option<String> {
    let token = value
        .trim()
        .trim_matches(|c: char| c == '"' || c == '\'' || c == ',' || c == ';')
        .trim_matches(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'));
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

fn extract_webui_token_from_json(text: &str) -> Option<String> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
        if let Some(token) = v.get("token").and_then(|t| t.as_str()) {
            return trim_token_like(token);
        }
    }

    // Fallback: tolerate non-standard JSON (e.g. trailing chars / comments).
    let key_pos = text.find("\"token\"")?;
    let after_key = &text[(key_pos + "\"token\"".len())..];
    let colon_pos = after_key.find(':')?;
    let after_colon = after_key[(colon_pos + 1)..].trim_start();
    let after_quote = after_colon.strip_prefix('"')?;
    let end_quote = after_quote.find('"')?;
    trim_token_like(&after_quote[..end_quote])
}

/// 获取 NapCat WebUI Token（优先日志，其次 webui.json）
pub async fn get_webui_token(container_name: &str) -> Option<String> {
    // 增加日志行数，因为 token 只在启动时输出一次
    let output = Command::new("docker")
        .args(["logs", "--tail", "5000", container_name])
        .output()
        .await
        .ok()?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let logs = format!("{}{}", stderr, stdout);

    // 去除 ANSI 转义码
    let clean_logs = strip_ansi_escapes::strip_str(&logs);

    // 查找 "WebUi Token: xxxxx" 格式的行（同一容器重启时可能出现多条，取最后一条）
    let mut last_token: Option<String> = None;
    for line in clean_logs.lines() {
        if line.contains("WebUi Token:")
            || line.contains("WebUI Token:")
            || line.contains("webui token:")
        {
            if let Some(token) = line.split("Token:").nth(1) {
                if let Some(token) = token.split_whitespace().next().and_then(trim_token_like) {
                    last_token = Some(token);
                }
            }
        }
    }

    if let Some(token) = last_token {
        info!("从容器 {} 获取到 WebUI Token（docker logs）", container_name);
        return Some(token);
    }

    // Newer NapCat images may not print WebUI token into docker logs anymore; try reading it from config.
    for path in ["/app/napcat/config/webui.json", "/app/napcat/config/webui.jsonc"] {
        let out = Command::new("docker")
            .args(["exec", container_name, "cat", path])
            .output()
            .await
            .ok();

        if let Some(out) = out {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout);
                if let Some(token) = extract_webui_token_from_json(&text) {
                    info!("从容器 {} 读取到 WebUI Token（{}）", container_name, path);
                    return Some(token);
                }
            }
        }
    }

    info!(
        "未能从容器 {} 获取 WebUI Token（logs/webui.json），日志长度: {} 字节",
        container_name,
        logs.len()
    );
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_token_from_json() {
        let json = r#"{ "token": "QCgjSiJTwUQd153MwH_vSlh4baXLOETtFfgqSX5oocs" }"#;
        let got = extract_webui_token_from_json(json).unwrap();
        assert_eq!(got, "QCgjSiJTwUQd153MwH_vSlh4baXLOETtFfgqSX5oocs");
    }

    #[test]
    fn extracts_token_from_nonstandard_text() {
        let text = r#"
            // comment
            { "token": "abc_DEF-123" } trailing
        "#;
        let got = extract_webui_token_from_json(text).unwrap();
        assert_eq!(got, "abc_DEF-123");
    }
}

/// 通过 Napcat WebUI HTTP API 检测登录状态并获取二维码
pub async fn napcat_login_monitor(state: SharedState) {
    info!("启动 NapCat 登录状态监控 (HTTP API)...");
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            error!("创建 HTTP Client 失败: {}", e);
            return;
        }
    };

    // 缓存每个 bot 的 credential
    let mut credentials: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    // 避免 WebUI 登录失败刷屏（相同错误 30s 内只输出一次 info）
    let mut last_login_failure: std::collections::HashMap<
        String,
        (i32, Option<String>, std::time::Instant),
    > = std::collections::HashMap::new();

    loop {
        // 获取所有运行中的 bot
        let bots_to_check: Vec<BotsToCheckItem> = state
            .bots
            .iter()
            .filter(|b| b.is_running && b.webui_port.is_some())
            .filter_map(|b| {
                b.webui_port.map(|p| {
                    (
                        b.id.clone(),
                        b.webui_host
                            .clone()
                            .unwrap_or_else(|| "127.0.0.1".to_string()),
                        p,
                        b.container_id.clone(),
                        b.webui_token.clone(),
                    )
                })
            })
            .collect();

        for (bot_id, webui_host, webui_port, container_id, saved_token) in bots_to_check {
            let base_url = format!("http://{}:{}", webui_host, webui_port);
            let container_name = container_id.as_ref().unwrap_or(&bot_id);

            // 如果没有 credential，先获取 token 并登录
            if !credentials.contains_key(&bot_id) {
                let login_url = format!("{}/api/auth/login", base_url);

                // WebUI Token 可能会随着容器重启而变化。
                // 这里先尝试 bots.json 中保存的 token；若失败再从容器日志提取 token 重试，避免 code=-1 死循环。
                let mut token_used: Option<String> = None;
                let mut credential: Option<String> = None;
                let mut last_err: Option<(i32, Option<String>)> = None;

                if let Some(token) = saved_token
                    .as_deref()
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                {
                    match webui_login(&client, &login_url, token).await {
                        Ok(auth_resp) => {
                            if auth_resp.code == 0 {
                                if let Some(data) = auth_resp.data {
                                    token_used = Some(token.to_string());
                                    credential = Some(data.Credential);
                                }
                            } else {
                                last_err = Some((auth_resp.code, auth_resp.message));
                            }
                        }
                        Err(e) => debug!("{} WebUI 登录请求失败: {}", bot_id, e),
                    }
                }

                if credential.is_none() {
                    if let Some(log_token) = get_webui_token(container_name).await {
                        let log_token = log_token.trim().to_string();
                        let already_tried = saved_token
                            .as_deref()
                            .map(str::trim)
                            .is_some_and(|t| t == log_token);

                        if !log_token.is_empty() && !already_tried {
                            match webui_login(&client, &login_url, &log_token).await {
                                Ok(auth_resp) => {
                                    if auth_resp.code == 0 {
                                        if let Some(data) = auth_resp.data {
                                            token_used = Some(log_token);
                                            credential = Some(data.Credential);
                                        }
                                    } else {
                                        last_err = Some((auth_resp.code, auth_resp.message));
                                    }
                                }
                                Err(e) => debug!("{} WebUI 登录请求失败: {}", bot_id, e),
                            }
                        }
                    }
                }

                if let (Some(token_used), Some(cred)) = (token_used, credential) {
                    info!("{} WebUI 登录成功", bot_id);
                    credentials.insert(bot_id.clone(), cred);
                    last_login_failure.remove(&bot_id);

                    // 持久化最新 token（避免下次启动继续使用旧 token 失败）
                    if let Some(mut bot) = state.bots.get_mut(&bot_id) {
                        if bot.webui_token.as_deref() != Some(token_used.as_str()) {
                            bot.webui_token = Some(token_used);
                            drop(bot);
                            save_bots(&state.bots);
                        }
                    }
                } else if let Some((code, message)) = last_err {
                    let should_log = !matches!(
                        last_login_failure.get(&bot_id),
                        Some((last_code, last_message, last_at))
                            if *last_code == code
                                && last_message.as_deref() == message.as_deref()
                                && last_at.elapsed() < Duration::from_secs(30)
                    );

                    if should_log {
                        if let Some(msg) = message.as_deref() {
                            info!("{} WebUI 登录失败: code={}, message={}", bot_id, code, msg);
                        } else {
                            info!("{} WebUI 登录失败: code={}", bot_id, code);
                        }
                        last_login_failure
                            .insert(bot_id.clone(), (code, message, std::time::Instant::now()));
                    } else {
                        debug!("{} WebUI 登录失败: code={}", bot_id, code);
                    }
                } else {
                    debug!("{} WebUI 登录失败：未能获取 WebUI Token 或请求失败", bot_id);
                }
            }

            // 使用 credential 检查 QQ 登录状态
            if let Some(credential) = credentials.get(&bot_id) {
                let status_url = format!("{}/api/QQLogin/CheckLoginStatus", base_url);

                match client
                    .post(&status_url)
                    .header("Authorization", format!("Bearer {}", credential))
                    .send()
                    .await
                {
                    Ok(resp) => {
                        if let Ok(status_resp) = resp.json::<QQLoginStatusResponse>().await {
                            if status_resp.code == 0 {
                                if let Some(data) = status_resp.data {
                                    if data.isLogin {
                                        // 已登录
                                        if let Some(mut bot) = state.bots.get_mut(&bot_id) {
                                            if !bot.is_connected {
                                                info!("机器人 {} QQ已登录", bot_id);
                                                bot.is_connected = true;
                                                drop(bot);
                                                save_bots(&state.bots);
                                            }
                                        }

                                        // 清除二维码
                                        *state.runtime.latest_qr.write().await = None;
                                        *state.runtime.latest_qr_image.write().await = None;
                                    } else {
                                        // 未登录
                                        if let Some(mut bot) = state.bots.get_mut(&bot_id) {
                                            if bot.is_connected {
                                                info!("机器人 {} QQ已断开", bot_id);
                                                bot.is_connected = false;
                                                drop(bot);
                                                save_bots(&state.bots);
                                            }
                                        }

                                        // 获取二维码 URL
                                        if let Some(qrcode_url) = data.qrcodeurl {
                                            if !qrcode_url.is_empty()
                                                && (qrcode_url.starts_with("http")
                                                    || qrcode_url.starts_with("data:image"))
                                            {
                                                let qrcode_url = qrcode_url.trim().to_string();
                                                let mut should_fetch = false;
                                                {
                                                    let mut latest_qr =
                                                        state.runtime.latest_qr.write().await;
                                                    if latest_qr.as_ref() != Some(&qrcode_url) {
                                                        info!("获取到 {} 的二维码 URL", bot_id);
                                                        *latest_qr = Some(qrcode_url.clone());
                                                        should_fetch = true;
                                                    }
                                                }

                                                if should_fetch {
                                                    let qr_image =
                                                        if qrcode_url.starts_with("data:image") {
                                                            Some(qrcode_url.clone())
                                                        } else {
                                                            let fetched =
                                                                fetch_qr_image_data_url(
                                                                    &client,
                                                                    &qrcode_url,
                                                                )
                                                                .await;
                                                            match fetched {
                                                                Some(v) => Some(v),
                                                                None => {
                                                                    crate::bot::generate_qr_png_data_url(
                                                                        &qrcode_url,
                                                                    )
                                                                }
                                                            }
                                                        };

                                                    *state.runtime.latest_qr_image.write().await =
                                                        qr_image.clone();

                                                    if let Some(img) = qr_image {
                                                        let current =
                                                            state.runtime.latest_qr.read().await;
                                                        if current.as_deref()
                                                            == Some(qrcode_url.as_str())
                                                        {
                                                            *state
                                                                .runtime
                                                                .latest_qr_image
                                                                .write()
                                                                .await = Some(img);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            } else if status_resp.code == -1 {
                                // Unauthorized - credential 过期，清除重新获取
                                info!("{} credential 过期，重新获取", bot_id);
                                credentials.remove(&bot_id);
                            } else {
                                debug!(
                                    "{} CheckLoginStatus 返回 code={}",
                                    bot_id, status_resp.code
                                );
                            }
                        }
                    }
                    Err(e) => {
                        debug!("{} 检查登录状态失败: {}", bot_id, e);
                    }
                }
            }
        }

        sleep(Duration::from_secs(3)).await;
    }
}

/// 同步 Docker 容器运行状态
pub async fn docker_status_sync_loop(state: SharedState) {
    info!("启动 Docker 状态同步循环...");
    loop {
        let output = Command::new("docker")
            .args(["ps", "-a", "--format", "{{.Names}}|{{.State}}"])
            .output()
            .await;

        if let Ok(out) = output {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let mut found_statuses: std::collections::HashMap<String, bool> =
                std::collections::HashMap::new();

            for line in stdout.lines() {
                let parts: Vec<&str> = line.split('|').collect();
                if parts.len() >= 2 {
                    let name = parts[0].trim_start_matches('/');
                    let status = parts[1].to_lowercase();
                    let is_running = status == "running";
                    found_statuses.insert(name.to_string(), is_running);
                }
            }

            let mut changed = false;
            for mut bot in state.bots.iter_mut() {
                if bot.platform.eq_ignore_ascii_case("discord") {
                    continue;
                }
                let target_name = bot.container_id.clone().unwrap_or(bot.id.clone());
                let actual_running = found_statuses.get(&target_name).copied().unwrap_or(false);

                if bot.is_running != actual_running {
                    info!("同步 {} 状态: is_running = {}", target_name, actual_running);
                    bot.is_running = actual_running;

                    if !actual_running {
                        bot.is_connected = false;
                    }
                    changed = true;
                }
            }

            if changed {
                save_bots(&state.bots);
            }
        }

        sleep(Duration::from_secs(2)).await;
    }
}
