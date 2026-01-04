use crate::models::SharedState;
use crate::plugin::verifier::sign_plugin;
use crate::plugin::{PluginManifest, PluginPackage, PluginVerifier};
use axum::extract::{Json, State};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use tracing::{info, warn};

use super::commands::register_plugin_commands;
use super::util::{allow_unsigned_plugins, is_safe_path_segment, json_error, json_success};

#[derive(serde::Deserialize)]
pub struct InstallPluginPayload {
    pub manifest: PluginManifest,
    pub code: String,
}

pub(super) async fn install_from_manifest_code(
    state: &SharedState,
    mut manifest: PluginManifest,
    code: String,
) -> Result<(), String> {
    if !is_safe_path_segment(&manifest.id) {
        return Err("Invalid plugin id (allowed: [A-Za-z0-9_.-], max 64)".to_string());
    }

    if state.plugins.get(&manifest.id).is_some() {
        return Err(format!("Plugin {} already installed", manifest.id));
    }

    let allow_unsigned = allow_unsigned_plugins();
    let signature_required = !manifest.builtin && !allow_unsigned;

    if signature_required && manifest.signature.is_none() {
        return Err(
            "Missing plugin signature (set NBOT_ALLOW_UNSIGNED_PLUGINS=true only for development)"
                .to_string(),
        );
    }

    if let Some(ref signature) = manifest.signature {
        let verifier = PluginVerifier::new()
            .map_err(|e| format!("Signature verifier misconfigured: {}", e))?;
        let ok = verifier.verify(&manifest.id, &manifest.version, code.as_bytes(), signature)?;
        if !ok {
            return Err("Invalid plugin signature".to_string());
        }
        info!("插件 {} 签名验证通过", manifest.id);
    } else if allow_unsigned {
        warn!("插件 {} 无签名：已允许（开发模式）", manifest.id);
    }

    let plugin_dir = state
        .plugins
        .plugins_dir()
        .join(match manifest.plugin_type {
            crate::plugin::PluginType::Bot => "bot",
            crate::plugin::PluginType::Platform => "platform",
        })
        .join(&manifest.id);

    if plugin_dir.exists() {
        return Err(format!(
            "Plugin directory already exists: {}",
            plugin_dir.to_string_lossy()
        ));
    }

    std::fs::create_dir_all(&plugin_dir)
        .map_err(|e| format!("Failed to create plugin dir: {}", e))?;

    // Write entry file (backward-compatible default is index.js).
    let mut entry = manifest.entry.trim().to_string();
    if entry.is_empty() {
        entry = "index.js".to_string();
        manifest.entry = entry.clone();
    }
    let mut code_path = plugin_dir.join(&entry);
    if code_path.is_dir() {
        code_path = code_path.join("index.js");
        let dir = entry.trim_end_matches('/');
        manifest.entry = format!("{}/index.js", dir);
    }
    if let Some(parent) = code_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create entry dir: {}", e))?;
    }
    std::fs::write(&code_path, &code).map_err(|e| format!("Failed to write code: {}", e))?;

    let manifest_path = plugin_dir.join("manifest.json");
    let manifest_content = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("Failed to serialize manifest: {}", e))?;
    std::fs::write(&manifest_path, manifest_content)
        .map_err(|e| format!("Failed to write manifest: {}", e))?;

    state
        .plugins
        .install(manifest.clone(), plugin_dir.to_string_lossy().to_string())
        .inspect_err(|_e| {
            let _ = std::fs::remove_dir_all(&plugin_dir);
        })?;

    let plugin = match state.plugins.get(&manifest.id) {
        Some(p) => p,
        None => {
            let _ = std::fs::remove_dir_all(&plugin_dir);
            return Err("Plugin registry update failed".to_string());
        }
    };

    if let Err(e) = state.plugin_manager.load(&plugin).await {
        warn!("插件 {} 安装后加载失败，将回滚: {}", plugin.manifest.id, e);
        let _ = state.plugins.uninstall(&plugin.manifest.id);
        return Err(format!("Plugin installed but failed to load: {}", e));
    }

    register_plugin_commands(&state.commands, &plugin);
    Ok(())
}

pub async fn install_plugin_handler(
    State(state): State<SharedState>,
    Json(payload): Json<InstallPluginPayload>,
) -> Json<serde_json::Value> {
    let plugin_id = payload.manifest.id.clone();
    match install_from_manifest_code(&state, payload.manifest, payload.code).await {
        Ok(_) => json_success(Some(&plugin_id)),
        Err(e) => json_error(e),
    }
}

/// 从 .nbp 包安装插件
#[derive(serde::Deserialize)]
pub struct InstallPackagePayload {
    pub package_b64: String, // Base64 编码的 .nbp 文件
}

pub async fn install_package_handler(
    State(state): State<SharedState>,
    Json(payload): Json<InstallPackagePayload>,
) -> Json<serde_json::Value> {
    // 解码 Base64
    let package_data = match BASE64.decode(&payload.package_b64) {
        Ok(data) => data,
        Err(e) => {
            return json_error(format!("Invalid base64: {}", e));
        }
    };

    // 解析插件包
    let package = match PluginPackage::from_bytes(&package_data) {
        Ok(pkg) => pkg,
        Err(e) => return json_error(e),
    };

    let plugin_id = package.manifest.id.clone();
    match install_from_package(&state, package).await {
        Ok(_) => json_success(Some(&plugin_id)),
        Err(e) => json_error(e),
    }
}

/// 开发工具：签名插件（仅用于测试）
#[derive(serde::Deserialize)]
pub struct SignPluginPayload {
    pub private_key: String,
    pub plugin_id: String,
    pub version: String,
    pub code: String,
}

pub async fn sign_plugin_handler(
    Json(payload): Json<SignPluginPayload>,
) -> Json<serde_json::Value> {
    match sign_plugin(
        &payload.private_key,
        &payload.plugin_id,
        &payload.version,
        payload.code.as_bytes(),
    ) {
        Ok(signature) => Json(serde_json::json!({ "status": "success", "signature": signature })),
        Err(e) => json_error(e),
    }
}

pub(super) async fn install_from_package(
    state: &SharedState,
    package: PluginPackage,
) -> Result<(), String> {
    let manifest = package.manifest;

    fn rel_to_path(rel: &str) -> std::path::PathBuf {
        let mut out = std::path::PathBuf::new();
        for seg in rel.split('/') {
            if !seg.is_empty() {
                out.push(seg);
            }
        }
        out
    }

    if !is_safe_path_segment(&manifest.id) {
        return Err("Invalid plugin id (allowed: [A-Za-z0-9_.-], max 64)".to_string());
    }

    if state.plugins.get(&manifest.id).is_some() {
        return Err(format!("Plugin {} already installed", manifest.id));
    }

    let allow_unsigned = allow_unsigned_plugins();
    let signature_required = !manifest.builtin && !allow_unsigned;

    if signature_required && manifest.signature.is_none() {
        return Err(
            "Missing plugin signature (set NBOT_ALLOW_UNSIGNED_PLUGINS=true only for development)"
                .to_string(),
        );
    }

    if let Some(ref signature) = manifest.signature {
        let verifier = PluginVerifier::new()
            .map_err(|e| format!("Signature verifier misconfigured: {}", e))?;

        let refs: Vec<(&str, &[u8])> = package
            .files
            .iter()
            .map(|f| (f.path.as_str(), f.data.as_slice()))
            .collect();

        let ok = verifier.verify_payload(&manifest.id, &manifest.version, &refs, signature)?;
        if !ok {
            // Backward-compat: allow legacy signatures for single-file packages that use index.js.
            let maybe_index = package
                .files
                .iter()
                .find(|f| f.path == "index.js")
                .map(|f| f.data.as_slice());
            let legacy_ok = if let Some(code) = maybe_index {
                verifier.verify(&manifest.id, &manifest.version, code, signature)?
            } else {
                false
            };
            if !legacy_ok {
                return Err("Invalid plugin signature".to_string());
            }
        }
        info!("插件 {} 签名验证通过", manifest.id);
    } else if allow_unsigned {
        warn!("插件 {} 无签名：已允许（开发模式）", manifest.id);
    }

    let plugin_dir = state
        .plugins
        .plugins_dir()
        .join(match manifest.plugin_type {
            crate::plugin::PluginType::Bot => "bot",
            crate::plugin::PluginType::Platform => "platform",
        })
        .join(&manifest.id);

    if plugin_dir.exists() {
        return Err(format!(
            "Plugin directory already exists: {}",
            plugin_dir.to_string_lossy()
        ));
    }

    std::fs::create_dir_all(&plugin_dir)
        .map_err(|e| format!("Failed to create plugin dir: {}", e))?;

    // manifest.json (user-writable config is stored here too)
    let manifest_path = plugin_dir.join("manifest.json");
    let manifest_content = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("Failed to serialize manifest: {}", e))?;
    std::fs::write(&manifest_path, manifest_content)
        .map_err(|e| format!("Failed to write manifest: {}", e))?;

    // Package files
    for f in package.files.iter() {
        // Paths are already sanitized in package parser; just create parents and write.
        let dest = plugin_dir.join(rel_to_path(&f.path));
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create dir {:?}: {}", parent, e))?;
        }
        std::fs::write(&dest, &f.data).map_err(|e| format!("Failed to write file {:?}: {}", dest, e))?;
    }

    state
        .plugins
        .install(manifest.clone(), plugin_dir.to_string_lossy().to_string())
        .inspect_err(|_e| {
            let _ = std::fs::remove_dir_all(&plugin_dir);
        })?;

    let plugin = match state.plugins.get(&manifest.id) {
        Some(p) => p,
        None => {
            let _ = std::fs::remove_dir_all(&plugin_dir);
            return Err("Plugin registry update failed".to_string());
        }
    };

    if let Err(e) = state.plugin_manager.load(&plugin).await {
        warn!("插件 {} 安装后加载失败，将回滚: {}", plugin.manifest.id, e);
        let _ = state.plugins.uninstall(&plugin.manifest.id);
        return Err(format!("Plugin installed but failed to load: {}", e));
    }

    register_plugin_commands(&state.commands, &plugin);
    Ok(())
}
