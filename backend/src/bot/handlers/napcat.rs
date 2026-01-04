use crate::models::SharedState;
use crate::persistence::save_bots;
use axum::extract::{Json, Path, State};
use base64::Engine as _;
use tracing::{info, warn};

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

pub async fn login_trigger_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    info!("手动触发机器人登录: {}", id);

    let (webui_host, webui_port, container_id, saved_token) = if let Some(bot) = state.bots.get(&id)
    {
        (
            bot.webui_host
                .clone()
                .unwrap_or_else(|| "127.0.0.1".to_string()),
            bot.webui_port,
            bot.container_id.clone(),
            bot.webui_token.clone(),
        )
    } else {
        return Json(
            serde_json::json!({ "status": "error", "message": "Bot not found", "qr": null }),
        );
    };

    let Some(port) = webui_port else {
        return Json(
            serde_json::json!({ "status": "error", "message": "Bot has no WebUI port configured", "qr": null }),
        );
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build();
    let client = match client {
        Ok(c) => c,
        Err(e) => {
            return Json(
                serde_json::json!({ "status": "error", "message": format!("Failed to create HTTP client: {}", e) }),
            );
        }
    };

    let base_url = format!("http://{}:{}", webui_host, port);
    let container_name = container_id.as_ref().unwrap_or(&id);

    let login_url = format!("{}/api/auth/login", base_url);

    #[derive(serde::Serialize)]
    struct LoginReq {
        hash: String,
    }
    #[derive(serde::Deserialize)]
    struct LoginResp {
        code: i32,
        data: Option<LoginData>,
        #[serde(default)]
        message: Option<String>,
    }
    #[derive(serde::Deserialize)]
    #[allow(non_snake_case)]
    struct LoginData {
        Credential: String,
    }

    let mut token_candidates: Vec<String> = Vec::new();
    if let Some(t) = saved_token
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        token_candidates.push(t.to_string());
    }
    if let Some(log_token) = crate::bot::monitor::get_webui_token(container_name).await {
        let log_token = log_token.trim().to_string();
        if !log_token.is_empty()
            && token_candidates
                .first()
                .is_none_or(|existing| existing.as_str() != log_token)
        {
            token_candidates.push(log_token);
        }
    }

    let mut token_used: Option<String> = None;
    let mut cred: Option<String> = None;
    let mut last_err: Option<(i32, Option<String>)> = None;

    for token in token_candidates {
        let hash = crate::bot::monitor::generate_password_hash(&token);
        match client
            .post(&login_url)
            .json(&LoginReq { hash })
            .send()
            .await
        {
            Ok(resp) => match resp.json::<LoginResp>().await {
                Ok(r) if r.code == 0 => {
                    if let Some(data) = r.data {
                        token_used = Some(token);
                        cred = Some(data.Credential);
                        break;
                    }
                }
                Ok(r) => {
                    last_err = Some((r.code, r.message));
                }
                Err(e) => warn!("NapCat WebUI auth response parse failed: {}", e),
            },
            Err(e) => warn!("NapCat WebUI auth request failed: {}", e),
        }
    }

    if let Some(token_used) = token_used.as_deref() {
        if let Some(mut bot) = state.bots.get_mut(&id) {
            if bot.webui_token.as_deref() != Some(token_used) {
                bot.webui_token = Some(token_used.to_string());
                drop(bot);
                save_bots(&state.bots);
            }
        }
    } else if let Some((code, message)) = last_err {
        warn!(
            "NapCat WebUI auth failed: code={}, message={:?}",
            code, message
        );
    }

    let Some(cred) = cred else {
        return Json(serde_json::json!({
            "status": "error",
            "message": "Failed to authenticate with NapCat WebUI",
            "qr": null
        }));
    };

    // 优先使用 CheckLoginStatus：既能判断是否已登录，也能拿到二维码 URL。
    let status_url = format!("{}/api/QQLogin/CheckLoginStatus", base_url);
    match client
        .post(&status_url)
        .header("Authorization", format!("Bearer {}", cred))
        .send()
        .await
    {
        Ok(resp) => {
            #[derive(serde::Deserialize)]
            struct StatusResp {
                code: i32,
                data: Option<StatusData>,
            }
            #[derive(serde::Deserialize)]
            #[allow(non_snake_case)]
            struct StatusData {
                #[serde(default)]
                isLogin: bool,
                #[serde(
                    default,
                    alias = "qrcodeUrl",
                    alias = "qrcodeURL",
                    alias = "qrcode_url",
                    alias = "qrCodeUrl",
                    alias = "qr_code_url"
                )]
                qrcodeurl: Option<String>,
            }

            if let Ok(status_resp) = resp.json::<StatusResp>().await {
                if status_resp.code == 0 {
                    if let Some(data) = status_resp.data {
                        if data.isLogin {
                            if let Some(mut bot) = state.bots.get_mut(&id) {
                                if !bot.is_connected {
                                    bot.is_connected = true;
                                    drop(bot);
                                    save_bots(&state.bots);
                                }
                            }
                            let mut latest_qr = state.runtime.latest_qr.write().await;
                            *latest_qr = None;
                            let mut latest_qr_image = state.runtime.latest_qr_image.write().await;
                            *latest_qr_image = None;
                            return Json(serde_json::json!({
                                "status": "success",
                                "message": "Already logged in",
                                "qr": null
                            }));
                        }
                        if let Some(qrcode_url) = data.qrcodeurl {
                            let qrcode_url = qrcode_url.trim().to_string();
                            if !qrcode_url.is_empty() {
                                let qr_image = if qrcode_url.starts_with("data:image") {
                                    Some(qrcode_url.clone())
                                } else {
                                    let fetched = fetch_qr_image_data_url(&client, &qrcode_url).await;
                                    match fetched {
                                        Some(v) => Some(v),
                                        None => crate::bot::generate_qr_png_data_url(&qrcode_url),
                                    }
                                };

                                {
                                    let mut latest_qr = state.runtime.latest_qr.write().await;
                                    *latest_qr = Some(qrcode_url.clone());
                                    let mut latest_qr_image =
                                        state.runtime.latest_qr_image.write().await;
                                    *latest_qr_image = qr_image.clone();
                                }

                                if let Some(img) = qr_image.as_ref() {
                                    let current = state.runtime.latest_qr.read().await;
                                    if current.as_deref() == Some(qrcode_url.as_str()) {
                                        let mut latest_qr_image =
                                            state.runtime.latest_qr_image.write().await;
                                        *latest_qr_image = Some(img.clone());
                                    }
                                }
                                return Json(serde_json::json!({
                                    "status": "success",
                                    "message": "QR code fetched",
                                    "qr": qrcode_url,
                                    "qr_image": qr_image
                                }));
                            }
                        }
                    }
                }
            }
        }
        Err(e) => {
            warn!("检查登录状态失败: {}", e);
        }
    }

    Json(serde_json::json!({
        "status": "error",
        "message": "Failed to fetch QR code from Napcat WebUI",
        "qr": null
    }))
}

pub async fn qr_handler(State(state): State<SharedState>) -> Json<serde_json::Value> {
    let qr = state.runtime.latest_qr.read().await.clone();
    let qr_image = state.runtime.latest_qr_image.read().await.clone();
    Json(serde_json::json!({ "qr": qr, "qr_image": qr_image }))
}

pub async fn qr_clear_handler(State(state): State<SharedState>) -> Json<serde_json::Value> {
    *state.runtime.latest_qr.write().await = None;
    *state.runtime.latest_qr_image.write().await = None;
    Json(serde_json::json!({ "status": "success" }))
}
