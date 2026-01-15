use crate::http::ApiError;
use crate::models::SharedState;
use axum::extract::{Json, State};
use std::io::Read;
use std::time::Duration;
use tracing::{info, warn};

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
    std::env::var("NBOT_MARKET_URL").unwrap_or_else(|_| "".to_string())
}

fn should_bootstrap_official_plugins() -> bool {
    // Backward-compatible name: "bootstrap" originally meant first-run install.
    // Now it covers both install and update-sync of official plugins.
    std::env::var("NBOT_MARKET_BOOTSTRAP_OFFICIAL_PLUGINS")
        .ok()
        .map(|v| {
            let v = v.trim();
            v.eq_ignore_ascii_case("1") || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(true)
}

fn should_force_official_plugin_update() -> bool {
    std::env::var("NBOT_MARKET_FORCE_UPDATE")
        .ok()
        .map(|v| {
            let v = v.trim();
            v.eq_ignore_ascii_case("1") || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

fn has_official_public_key_configured() -> bool {
    std::env::var("NBOT_OFFICIAL_PUBLIC_KEY_B64")
        .ok()
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
}

fn official_plugin_ids() -> Vec<&'static str> {
    // Keep this list tight: only plugins we maintain and sign as "official".
    // This prevents accidentally auto-installing community plugins.
    vec![
        "smart-assist",
        "member-verify",
        "whitelist",
        "ai-analysis",
        "mc-log-analysis",
        "cooldown",
        "greytip-guard",
        "jrys",
        "like",
    ]
}

fn parse_version_parts(v: &str) -> Option<Vec<u64>> {
    // Accept formats like "v2.2.33", "2.2.33", "2.2.33-beta.1".
    let v = v.trim().trim_start_matches('v').trim_start_matches('V').trim();
    if v.is_empty() {
        return None;
    }
    let core = v.split_whitespace().next().unwrap_or(v);
    let core = core.split('-').next().unwrap_or(core);
    let mut parts: Vec<u64> = Vec::new();
    for seg in core.split('.') {
        if seg.is_empty() {
            return None;
        }
        let n: u64 = seg.parse().ok()?;
        parts.push(n);
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts)
    }
}

fn is_remote_newer(local: &str, remote: &str) -> Option<bool> {
    let a = parse_version_parts(local)?;
    let b = parse_version_parts(remote)?;
    let max_len = std::cmp::max(a.len(), b.len());
    for i in 0..max_len {
        let av = *a.get(i).unwrap_or(&0);
        let bv = *b.get(i).unwrap_or(&0);
        if bv > av {
            return Some(true);
        }
        if bv < av {
            return Some(false);
        }
    }
    Some(false)
}

async fn download_market_package(
    client: &reqwest::Client,
    base: &str,
    plugin_id: &str,
) -> Result<Vec<u8>, String> {
    let url = format!(
        "{}/api/plugins/{}/download",
        base.trim_end_matches('/'),
        plugin_id
    );
    let resp = client
        .get(url)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("Market download failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("Market download failed: HTTP {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Market download read failed: {}", e))?;
    Ok(bytes.to_vec())
}

#[derive(serde::Serialize)]
pub struct SyncOfficialPluginsReport {
    pub installed: usize,
    pub updated: usize,
    pub skipped: usize,
    pub failed: usize,
}

async fn sync_official_plugins(
    state: &SharedState,
    force_update_override: Option<bool>,
) -> Result<SyncOfficialPluginsReport, String> {
    let base = market_base_url();
    if base.trim().is_empty() {
        return Err("Market 未配置：请设置环境变量 NBOT_MARKET_URL".to_string());
    }

    // Official plugins are signed; without a configured key we can't install/update them.
    // (In dev, NBOT_ALLOW_UNSIGNED_PLUGINS=true can bypass this check.)
    if !has_official_public_key_configured() && !super::util::allow_unsigned_plugins() {
        return Err(
            "未配置官方插件验签公钥：请设置 NBOT_OFFICIAL_PUBLIC_KEY_B64（或仅在开发环境启用 NBOT_ALLOW_UNSIGNED_PLUGINS=true）"
                .to_string(),
        );
    }

    let wanted = official_plugin_ids();
    let force_update = force_update_override.unwrap_or_else(should_force_official_plugin_update);

    let url = format!("{}/api/plugins", base.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let resp = match client
        .get(url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return Err(format!("Market 插件列表请求失败: {}", e));
        }
    };
    if !resp.status().is_success() {
        return Err(format!("Market 插件列表返回错误: HTTP {}", resp.status()));
    }
    let list = match resp.json::<Vec<MarketPluginInfo>>().await {
        Ok(v) => v,
        Err(e) => {
            return Err(format!("Market 插件列表解析失败: {}", e));
        }
    };

    let mut install_order: Vec<MarketPluginInfo> = list
        .into_iter()
        .filter(|p| wanted.iter().any(|id| *id == p.id))
        .collect();

    // Install in a deterministic order (use the list above as priority order).
    install_order.sort_by_key(|p| {
        wanted
            .iter()
            .position(|id| *id == p.id)
            .unwrap_or(usize::MAX)
    });

    if install_order.is_empty() {
        return Err(
            "Market 中未发现任何官方插件：请确认 nbot-site 已启动并已导入 official-plugins"
                .to_string(),
        );
    }

    let mut report = SyncOfficialPluginsReport {
        installed: 0,
        updated: 0,
        skipped: 0,
        failed: 0,
    };

    for p in install_order {
        let plugin_id = p.id.as_str();
        let local = state.plugins.get(plugin_id);
        let local_enabled = local.as_ref().map(|x| x.enabled).unwrap_or(true);
        let local_version = local
            .as_ref()
            .map(|x| x.manifest.version.as_str())
            .unwrap_or("");
        let local_config = local.as_ref().map(|x| x.manifest.config.clone());

        let should_install_or_update = if local.is_none() {
            true
        } else if force_update && p.version != local_version {
            true
        } else if let Some(newer) = is_remote_newer(local_version, &p.version) {
            newer
        } else {
            // Unknown version format; only update when force flag is on.
            false
        };

        if !should_install_or_update {
            report.skipped += 1;
            continue;
        }

        let bytes = match download_market_package(&client, &base, plugin_id).await {
            Ok(b) => b,
            Err(e) => {
                warn!("Market sync: download failed {}: {}", plugin_id, e);
                report.failed += 1;
                continue;
            }
        };

        let parsed = match parse_any_nbp_package(bytes.as_slice()) {
            Ok(v) => v,
            Err(e) => {
                warn!("Market sync: invalid package {}: {}", plugin_id, e);
                report.failed += 1;
                continue;
            }
        };

        // Sanity: ensure we are installing what we asked for.
        let manifest_id = match &parsed {
            ParsedMarketPackage::Native(pkg) => pkg.manifest.id.as_str(),
            ParsedMarketPackage::Legacy { manifest, .. } => manifest.id.as_str(),
        };
        if manifest_id != plugin_id {
            warn!(
                "Market sync: plugin id mismatch (wanted={}, got={})",
                plugin_id, manifest_id
            );
            report.failed += 1;
            continue;
        }

        if local.is_some() {
            info!("Market: updating plugin {} ({} -> {})", plugin_id, local_version, p.version);
            // Unload runtime + remove commands before replacing code.
            if state.plugin_manager.is_loaded(plugin_id) {
                if let Err(e) = state.plugin_manager.unload(plugin_id).await {
                    warn!("Market update: unload failed {}: {}", plugin_id, e);
                }
            }
            state.commands.unregister_plugin_commands(plugin_id);
            if let Err(e) = state.plugins.uninstall(plugin_id) {
                warn!("Market update: uninstall failed {}: {}", plugin_id, e);
                report.failed += 1;
                continue;
            }
        } else {
            info!("Market: installing plugin {} ({})", plugin_id, p.version);
        }

        let install_ok = match parsed {
            ParsedMarketPackage::Native(pkg) => install_from_package(state, pkg).await,
            ParsedMarketPackage::Legacy { manifest, code } => {
                install_from_manifest_code(state, manifest, code).await
            }
        };
        if let Err(e) = install_ok {
            warn!("Market sync: install failed {}: {}", plugin_id, e);
            report.failed += 1;
            continue;
        }

        if local.is_some() {
            report.updated += 1;
        } else {
            report.installed += 1;
        }

        // Restore user config + enabled state after install/update.
        if let Some(cfg) = local_config {
            if state.plugin_manager.is_loaded(plugin_id) {
                if let Err(e) = state.plugin_manager.update_config(plugin_id, cfg.clone()).await {
                    warn!("Market sync: runtime config update failed {}: {}", plugin_id, e);
                }
            }
            if let Err(e) = state.plugins.update_config(plugin_id, cfg.clone()) {
                warn!("Market sync: persisted config update failed {}: {}", plugin_id, e);
            }
        }

        if !local_enabled {
            if state.plugin_manager.is_loaded(plugin_id) {
                if let Err(e) = state.plugin_manager.unload(plugin_id).await {
                    warn!("Market sync: unload after install failed {}: {}", plugin_id, e);
                }
            }
            if let Err(e) = state.plugins.disable(plugin_id) {
                warn!("Market sync: disable failed {}: {}", plugin_id, e);
            }
            state.commands.unregister_plugin_commands(plugin_id);
        } else {
            let _ = state.plugins.enable(plugin_id);
            if let Some(plugin) = state.plugins.get(plugin_id) {
                if plugin.enabled {
                    super::commands::register_plugin_commands(&state.commands, &plugin);
                }
            }
        }
    }

    Ok(report)
}

pub async fn bootstrap_official_plugins_startup(state: &SharedState) {
    let base = market_base_url();
    if base.trim().is_empty() {
        return;
    }
    if !should_bootstrap_official_plugins() {
        return;
    }

    match sync_official_plugins(state, None).await {
        Ok(report) => {
            info!(
                "Market official plugins synced (installed={}, updated={}, skipped={}, failed={})",
                report.installed, report.updated, report.skipped, report.failed
            );
        }
        Err(e) => {
            warn!("Market official plugins sync skipped/failed: {}", e);
        }
    }
}

#[derive(serde::Deserialize)]
pub struct SyncOfficialPluginsPayload {
    #[serde(default)]
    pub force_update: Option<bool>,
}

pub async fn sync_official_plugins_handler(
    State(state): State<SharedState>,
    Json(payload): Json<SyncOfficialPluginsPayload>,
) -> Json<serde_json::Value> {
    match sync_official_plugins(&state, payload.force_update).await {
        Ok(report) => Json(serde_json::json!({
            "status": "success",
            "report": report,
        })),
        Err(e) => json_error(e),
    }
}

pub async fn list_market_plugins_handler(
    State(_state): State<SharedState>,
) -> Result<Json<Vec<MarketPluginInfo>>, ApiError> {
    let base = market_base_url();
    if base.trim().is_empty() {
        // Market is optional; return empty list when not configured.
        return Ok(Json(Vec::new()));
    }
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
    if base.trim().is_empty() {
        return json_error("Market 未配置：请设置环境变量 NBOT_MARKET_URL");
    }
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
