use crate::http::ApiError;
use crate::models::SharedState;
use axum::extract::{Json, State};
use std::io::Read;
use std::time::Duration;

use super::install::{install_from_manifest_code, install_from_package};
use super::util::{json_error, json_success};

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
pub struct MarketPluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub plugin_type: String,
}

#[derive(serde::Deserialize)]
pub struct InstallFromMarketPayload {
    pub plugin_id: String,
}

fn market_base_url() -> String {
    std::env::var("NBOT_MARKET_URL").unwrap_or_else(|_| "https://nbot.aspect.icu".to_string())
}

pub async fn list_market_plugins_handler(
    State(_state): State<SharedState>,
) -> Result<Json<Vec<MarketPluginInfo>>, ApiError> {
    let base = market_base_url();
    let url = format!("{}/api/plugins", base.trim_end_matches('/'));

    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| ApiError::bad_gateway(format!("Market 请求失败: {}", e)))?;

    if !resp.status().is_success() {
        return Err(ApiError::bad_gateway(format!(
            "Market 返回错误: HTTP {}",
            resp.status()
        )));
    }

    let list = resp
        .json::<Vec<MarketPluginInfo>>()
        .await
        .map_err(|e| ApiError::bad_gateway(format!("Market 响应解析失败: {}", e)))?;
    Ok(Json(list))
}

pub async fn install_from_market_handler(
    State(state): State<SharedState>,
    Json(payload): Json<InstallFromMarketPayload>,
) -> Json<serde_json::Value> {
    let plugin_id = payload.plugin_id.trim();
    if plugin_id.is_empty() {
        return json_error("plugin_id is required");
    }

    let base = market_base_url();
    let url = format!(
        "{}/api/plugins/{}/download",
        base.trim_end_matches('/'),
        plugin_id
    );

    let client = reqwest::Client::new();
    let resp = match client
        .get(url)
        .timeout(Duration::from_secs(30))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return json_error(format!("Market download failed: {}", e));
        }
    };

    if !resp.status().is_success() {
        return json_error(format!("Market download failed: HTTP {}", resp.status()));
    }

    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => {
            return json_error(format!("Market download read failed: {}", e));
        }
    };

    match parse_any_nbp_package(bytes.as_ref()) {
        Ok(ParsedMarketPackage::Native(pkg)) => {
            let plugin_id = pkg.manifest.id.clone();
            match install_from_package(&state, pkg).await {
                Ok(_) => json_success(Some(&plugin_id)),
                Err(e) => json_error(e),
            }
        }
        Ok(ParsedMarketPackage::Legacy { manifest, code }) => {
            let plugin_id = manifest.id.clone();
            match install_from_manifest_code(&state, manifest, code).await {
                Ok(_) => json_success(Some(&plugin_id)),
                Err(e) => json_error(e),
            }
        }
        Err(e) => json_error(e),
    }
}

enum ParsedMarketPackage {
    Native(crate::plugin::PluginPackage),
    Legacy { manifest: crate::plugin::PluginManifest, code: String },
}

fn parse_any_nbp_package(data: &[u8]) -> Result<ParsedMarketPackage, String> {
    // Fast path: backend-native package format.
    if let Ok(pkg) = crate::plugin::PluginPackage::from_bytes(data) {
        return Ok(ParsedMarketPackage::Native(pkg));
    }

    // Compatibility: market-server legacy manifest.json schema.
    let (manifest_json, code) = extract_nbp_files(data)?;
    let manifest = normalize_manifest(manifest_json)?;
    Ok(ParsedMarketPackage::Legacy { manifest, code })
}

fn extract_nbp_files(data: &[u8]) -> Result<(serde_json::Value, String), String> {
    use flate2::read::GzDecoder;
    use std::io::Cursor;
    use tar::Archive;

    let cursor = Cursor::new(data);
    let gz = GzDecoder::new(cursor);
    let mut archive = Archive::new(gz);

    let mut manifest_json: Option<serde_json::Value> = None;
    let mut code: Option<String> = None;

    for entry in archive
        .entries()
        .map_err(|e| format!("Failed to read archive: {}", e))?
    {
        let mut entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry
            .path()
            .map_err(|e| format!("Failed to get path: {}", e))?;
        let path_str = path.to_string_lossy();

        if path_str.ends_with("manifest.json") {
            let mut content = String::new();
            entry
                .read_to_string(&mut content)
                .map_err(|e| format!("Failed to read manifest: {}", e))?;
            manifest_json = Some(
                serde_json::from_str(&content)
                    .map_err(|e| format!("Invalid manifest JSON: {}", e))?,
            );
        } else if path_str.ends_with("index.js") {
            let mut content = String::new();
            entry
                .read_to_string(&mut content)
                .map_err(|e| format!("Failed to read code: {}", e))?;
            code = Some(content);
        } else {
            // ignore
            let mut _sink = Vec::new();
            let _ = entry.read_to_end(&mut _sink);
        }
    }

    let manifest_json = manifest_json.ok_or("Missing manifest.json")?;
    let code = code.ok_or("Missing index.js")?;
    Ok((manifest_json, code))
}

fn normalize_manifest(
    manifest_json: serde_json::Value,
) -> Result<crate::plugin::PluginManifest, String> {
    // Try backend manifest first (for forwards-compat with updated market server).
    if let Ok(m) = serde_json::from_value::<crate::plugin::PluginManifest>(manifest_json.clone()) {
        return Ok(m);
    }

    let mut obj = match manifest_json {
        serde_json::Value::Object(o) => o,
        _ => return Err("Invalid manifest format".to_string()),
    };

    // Map market-server `plugin_type` -> backend `type` (lowercase).
    if !obj.contains_key("type") {
        let plugin_type = obj
            .remove("plugin_type")
            .or_else(|| obj.remove("pluginType"));

        if let Some(pt) = plugin_type {
            let pt = pt
                .as_str()
                .ok_or_else(|| "plugin_type 必须为字符串".to_string())?;
            let t = match pt {
                "Bot" | "bot" | "module" => "bot",
                "Platform" | "platform" | "plugin" => "platform",
                other => {
                    return Err(format!("Unknown plugin_type: {}", other));
                }
            };
            obj.insert("type".to_string(), serde_json::Value::String(t.to_string()));
        }
    }

    // Provide defaults expected by backend manifest schema.
    obj.entry("builtin".to_string())
        .or_insert(serde_json::Value::Bool(false));
    obj.entry("commands".to_string())
        .or_insert(serde_json::Value::Array(Vec::new()));

    if let Some(v) = obj.remove("config_schema") {
        obj.insert("configSchema".to_string(), v);
    }
    obj.entry("configSchema".to_string())
        .or_insert(serde_json::Value::Array(Vec::new()));

    obj.entry("config".to_string())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));

    serde_json::from_value::<crate::plugin::PluginManifest>(serde_json::Value::Object(obj))
        .map_err(|e| format!("Invalid manifest: {}", e))
}
