use axum::extract::State;
use axum::http::{header, HeaderValue, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use rand_core::{OsRng, RngCore};
use std::path::Path;
use std::sync::Arc;
use tracing::{info, warn};

const SESSION_COOKIE_KEY: &str = "nbot_session";

#[derive(Clone)]
pub struct AuthState {
    pub api_token: String,
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

fn extract_bearer_token(auth_header: &str) -> Option<&str> {
    let auth_header = auth_header.trim();
    let (scheme, token) = auth_header.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

fn extract_cookie_value<'a>(cookie_header: &'a str, key: &str) -> Option<&'a str> {
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some((k, v)) = part.split_once('=') {
            if k.trim() == key {
                let v = v.trim();
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
    }
    None
}

fn build_session_cookie(token: &str) -> String {
    format!("{SESSION_COOKIE_KEY}={token}; Path=/; Max-Age=31536000; SameSite=Lax; HttpOnly")
}

pub async fn require_api_token(
    State(state): State<Arc<AuthState>>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    if req.method() == axum::http::Method::OPTIONS {
        return next.run(req).await;
    }

    let provided = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(extract_bearer_token);

    let provided = provided.or_else(|| {
        req.headers()
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .and_then(|c| extract_cookie_value(c, SESSION_COOKIE_KEY))
    });

    match provided {
        Some(token) if constant_time_eq(token, &state.api_token) => {
            let mut resp = next.run(req).await;
            if let Ok(cookie) = HeaderValue::from_str(&build_session_cookie(&state.api_token)) {
                resp.headers_mut().append(header::SET_COOKIE, cookie);
            }
            resp
        }
        _ => (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"))],
            axum::Json(serde_json::json!({
                "status": "error",
                "message": "Unauthorized: missing or invalid API token"
            })),
        )
            .into_response(),
    }
}

pub fn load_or_create_api_token(data_dir: &str) -> String {
    if let Ok(token) = std::env::var("NBOT_API_TOKEN") {
        let trimmed = token.trim().to_string();
        if !trimmed.is_empty() {
            info!("使用环境变量 NBOT_API_TOKEN 作为 API Token");
            return trimmed;
        }
    }

    let state_dir = Path::new(data_dir).join("state");
    let token_path = state_dir.join("api_token.txt");

    if let Ok(existing) = std::fs::read_to_string(&token_path) {
        let token = existing.trim().to_string();
        if !token.is_empty() {
            info!("已从 {:?} 加载 API Token", token_path);
            return token;
        }
    }

    if let Err(e) = std::fs::create_dir_all(&state_dir) {
        warn!("无法创建状态目录 {:?}: {}", state_dir, e);
    }
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);

    if let Err(e) = std::fs::write(&token_path, &token) {
        warn!("无法写入 API Token 到 {:?}: {}", token_path, e);
    } else {
        info!(
            "已生成新的 API Token 并写入 {:?}，请在 WebUI 中填写或设置 NBOT_API_TOKEN",
            token_path
        );
    }

    token
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_cookie_value_finds_key() {
        let header = "foo=1; nbot_session=abc123; bar=2";
        assert_eq!(
            extract_cookie_value(header, SESSION_COOKIE_KEY),
            Some("abc123")
        );
    }

    #[test]
    fn extract_cookie_value_ignores_malformed_parts() {
        let header = "foo=1; malformed; nbot_session=xyz; bar=2";
        assert_eq!(
            extract_cookie_value(header, SESSION_COOKIE_KEY),
            Some("xyz")
        );
    }

    #[test]
    fn build_session_cookie_has_expected_attrs() {
        let cookie = build_session_cookie("tkn");
        assert!(cookie.contains("nbot_session=tkn"));
        assert!(cookie.contains("Path=/"));
        assert!(cookie.contains("Max-Age="));
        assert!(cookie.contains("SameSite="));
        assert!(cookie.contains("HttpOnly"));
    }
}
