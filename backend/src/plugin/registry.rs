use crate::plugin::types::{InstalledPlugin, PluginManifest};
use dashmap::DashMap;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

pub struct PluginRegistry {
    plugins: DashMap<String, InstalledPlugin>,
    plugins_dir: PathBuf,
    state_file: PathBuf,
}

impl PluginRegistry {
    pub fn new(data_dir: &str) -> Self {
        let plugins_dir = PathBuf::from(data_dir).join("plugins");
        if let Err(e) = std::fs::create_dir_all(&plugins_dir) {
            warn!("创建插件目录失败 {:?}: {}", plugins_dir, e);
        }
        if let Err(e) = std::fs::create_dir_all(plugins_dir.join("bot")) {
            warn!("创建 bot 插件目录失败: {}", e);
        }
        if let Err(e) = std::fs::create_dir_all(plugins_dir.join("platform")) {
            warn!("创建 platform 插件目录失败: {}", e);
        }

        let state_file = PathBuf::from(data_dir).join("state").join("plugins.json");

        let registry = Self {
            plugins: DashMap::new(),
            plugins_dir,
            state_file,
        };

        // When the official market is configured, prefer market-distributed plugins over bundled seed plugins.
        // This allows the bot image to stay thin and keeps plugin updates centralized in nbot-site.
        if registry.should_use_seed_builtin_plugins() {
            // In Docker mode, built-in plugins live under /app/data.seed and are copied into the persisted data dir
            // only once. This sync step updates built-in plugin code/manifest (without clobbering user config)
            // so upgrades actually take effect for existing installs.
            registry.sync_builtin_plugins_from_seed();
            registry.scan_builtin_plugins();
        } else {
            info!("已禁用内置 seed 插件（将从 Market 安装/更新）");
        }
        registry.load_state();
        registry
    }

    fn should_use_seed_builtin_plugins(&self) -> bool {
        let forced = std::env::var("NBOT_USE_SEED_BUILTIN_PLUGINS")
            .ok()
            .map(|v| {
                let v = v.trim();
                v.eq_ignore_ascii_case("1")
                    || v.eq_ignore_ascii_case("true")
                    || v.eq_ignore_ascii_case("yes")
            })
            .unwrap_or(false);
        if forced {
            return true;
        }

        let disabled = std::env::var("NBOT_DISABLE_SEED_BUILTIN_PLUGINS")
            .ok()
            .map(|v| {
                let v = v.trim();
                v.eq_ignore_ascii_case("1")
                    || v.eq_ignore_ascii_case("true")
                    || v.eq_ignore_ascii_case("yes")
            })
            .unwrap_or(false);
        if disabled {
            return false;
        }

        // Default: if Market is configured, don't use seed built-ins.
        let market = std::env::var("NBOT_MARKET_URL").unwrap_or_else(|_| "".to_string());
        market.trim().is_empty()
    }

    fn detect_seed_data_dir() -> Option<PathBuf> {
        let mut candidates: Vec<PathBuf> = Vec::new();

        if let Ok(v) = std::env::var("NBOT_SEED_DATA_DIR") {
            let p = PathBuf::from(v.trim());
            if !p.as_os_str().is_empty() {
                candidates.push(p);
            }
        }
        if let Ok(v) = std::env::var("NBOT_SEED_DIR") {
            let p = PathBuf::from(v.trim());
            if !p.as_os_str().is_empty() {
                candidates.push(p);
            }
        }

        // Dockerfile puts built-in data here; entrypoint copies it into the persistent volume.
        candidates.push(PathBuf::from("/app/data.seed"));
        candidates.push(PathBuf::from("data.seed"));

        candidates.into_iter().find(|p| p.exists() && p.is_dir())
    }

    fn copy_dir_overwrite(src: &Path, dest: &Path) -> Result<(), String> {
        if !src.exists() {
            return Ok(());
        }
        std::fs::create_dir_all(dest).map_err(|e| format!("创建目录失败 {:?}: {}", dest, e))?;

        let entries =
            std::fs::read_dir(src).map_err(|e| format!("读取目录失败 {:?}: {}", src, e))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let dest_path = dest.join(&name);
            if path.is_dir() {
                Self::copy_dir_overwrite(&path, &dest_path)?;
                continue;
            }
            if path.is_file() {
                std::fs::copy(&path, &dest_path)
                    .map_err(|e| format!("复制文件失败 {:?} -> {:?}: {}", path, dest_path, e))?;
            }
        }
        Ok(())
    }

    fn read_json_value(path: &Path) -> Option<Value> {
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str::<Value>(&content).ok()
    }

    fn sync_builtin_plugins_from_seed(&self) {
        let Some(seed_dir) = Self::detect_seed_data_dir() else {
            return;
        };

        let seed_plugins_dir = seed_dir.join("plugins");
        if !seed_plugins_dir.is_dir() {
            return;
        }

        for subdir in ["bot", "platform"] {
            let seed_root = seed_plugins_dir.join(subdir);
            if !seed_root.is_dir() {
                continue;
            }

            let dest_root = self.plugins_dir.join(subdir);
            if let Err(e) = std::fs::create_dir_all(&dest_root) {
                warn!("创建插件目录失败 {:?}: {}", dest_root, e);
                continue;
            }

            let entries = match std::fs::read_dir(&seed_root) {
                Ok(e) => e,
                Err(e) => {
                    warn!("读取 seed 插件目录失败 {:?}: {}", seed_root, e);
                    continue;
                }
            };

            for entry in entries.flatten() {
                let seed_path = entry.path();
                if !seed_path.is_dir() {
                    continue;
                }

                let seed_manifest_path = seed_path.join("manifest.json");
                if !seed_manifest_path.is_file() {
                    continue;
                }

                let seed_manifest: PluginManifest =
                    match std::fs::read_to_string(&seed_manifest_path)
                        .ok()
                        .and_then(|c| serde_json::from_str::<PluginManifest>(&c).ok())
                    {
                        Some(m) => m,
                        None => {
                            warn!("解析 seed manifest 失败: {:?}", seed_manifest_path);
                            continue;
                        }
                    };

                if !seed_manifest.builtin {
                    continue;
                }

                let folder_name = entry.file_name();
                let dest_path = dest_root.join(&folder_name);
                let dest_manifest_path = dest_path.join("manifest.json");

                let dest_val = Self::read_json_value(&dest_manifest_path);
                let dest_version = dest_val
                    .as_ref()
                    .and_then(|v| v.get("version"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let dest_config = dest_val
                    .as_ref()
                    .and_then(|v| v.get("config"))
                    .cloned()
                    .filter(|v| !v.is_null());

                let need_update = dest_version
                    .as_deref()
                    .map(|v| v != seed_manifest.version.as_str())
                    .unwrap_or(true);
                if !need_update {
                    continue;
                }

                if let Err(e) = std::fs::create_dir_all(&dest_path) {
                    warn!("创建插件目录失败 {:?}: {}", dest_path, e);
                    continue;
                }

                if let Err(e) = Self::copy_dir_overwrite(&seed_path, &dest_path) {
                    warn!("同步内置插件失败 {:?} -> {:?}: {}", seed_path, dest_path, e);
                    continue;
                }

                let mut merged_manifest = seed_manifest.clone();
                if let Some(cfg) = dest_config {
                    merged_manifest.config = cfg;
                }

                match serde_json::to_string_pretty(&merged_manifest) {
                    Ok(content) => {
                        if let Err(e) = std::fs::write(&dest_manifest_path, content) {
                            warn!("写入插件 manifest 失败 {:?}: {}", dest_manifest_path, e);
                            continue;
                        }
                    }
                    Err(e) => {
                        warn!(
                            "序列化插件 manifest 失败（未写入到磁盘）: {}: {}",
                            merged_manifest.id, e
                        );
                        continue;
                    }
                };

                info!(
                    "已同步内置插件 {} ({} -> {})",
                    merged_manifest.id,
                    dest_version.as_deref().unwrap_or("<none>"),
                    merged_manifest.version
                );
            }
        }
    }

    /// 扫描并加载内置插件
    fn scan_builtin_plugins(&self) {
        for subdir in ["bot", "platform"] {
            let dir = self.plugins_dir.join(subdir);
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        let manifest_path = path.join("manifest.json");
                        if manifest_path.exists() {
                            if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                                if let Ok(manifest) =
                                    serde_json::from_str::<PluginManifest>(&content)
                                {
                                    if manifest.builtin && !self.plugins.contains_key(&manifest.id)
                                    {
                                        let plugin = InstalledPlugin {
                                            manifest: manifest.clone(),
                                            enabled: false,
                                            path: path.to_string_lossy().to_string(),
                                        };
                                        self.plugins.insert(manifest.id.clone(), plugin);
                                        info!("加载内置插件: {}", manifest.id);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn load_state(&self) {
        if let Ok(content) = std::fs::read_to_string(&self.state_file) {
            if let Ok(plugins) = serde_json::from_str::<Vec<InstalledPlugin>>(&content) {
                let mut updated_builtin_manifest = false;
                for plugin in plugins {
                    // For builtin plugins, keep user config/enabled but refresh manifest fields from disk
                    // (so shipped updates take effect even if plugins.json is stale).
                    if plugin.manifest.builtin {
                        let manifest_path = PathBuf::from(&plugin.path).join("manifest.json");
                        if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                            if let Ok(mut disk_manifest) =
                                serde_json::from_str::<PluginManifest>(&content)
                            {
                                if disk_manifest.builtin && disk_manifest.id == plugin.manifest.id {
                                    let mut user_config = plugin.manifest.config.clone();
                                    // Builtin plugin config migrations: prune deprecated keys to avoid "double switches".
                                    if plugin.manifest.id == "greytip-guard" {
                                        if let Some(obj) = user_config.as_object_mut() {
                                            if obj.remove("enabled").is_some() {
                                                updated_builtin_manifest = true;
                                            }
                                        }
                                    }
                                    if plugin.manifest.id == "member-verify" {
                                        if let Some(obj) = user_config.as_object_mut() {
                                            if obj.remove("enabled_groups").is_some() {
                                                updated_builtin_manifest = true;
                                            }
                                        }
                                    }
                                    if plugin.manifest.id == "smart-assist" {
                                        if let Some(obj) = user_config.as_object_mut() {
                                            if obj.remove("enabled_groups").is_some() {
                                                updated_builtin_manifest = true;
                                            }
                                        }
                                    }
                                    disk_manifest.config = user_config;

                                    if plugin.manifest.version != disk_manifest.version
                                        || plugin.manifest.description != disk_manifest.description
                                        || plugin.manifest.commands != disk_manifest.commands
                                    {
                                        updated_builtin_manifest = true;
                                    }

                                    let mut merged = plugin.clone();
                                    merged.manifest = disk_manifest;
                                    self.plugins.insert(merged.manifest.id.clone(), merged);
                                    continue;
                                }
                            }
                        }
                    }

                    self.plugins.insert(plugin.manifest.id.clone(), plugin);
                }
                info!("从状态文件加载了 {} 个插件", self.plugins.len());
                if updated_builtin_manifest {
                    self.save_state();
                }
            }
        }
    }

    pub fn save_state(&self) {
        let plugins: Vec<InstalledPlugin> =
            self.plugins.iter().map(|p| p.value().clone()).collect();
        if let Ok(content) = serde_json::to_string_pretty(&plugins) {
            if let Some(parent) = self.state_file.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    warn!("创建插件状态目录失败 {:?}: {}", parent, e);
                }
            }
            if let Err(e) = std::fs::write(&self.state_file, content) {
                warn!("写入插件状态文件失败 {:?}: {}", self.state_file, e);
            }
        } else {
            warn!("序列化插件状态失败（plugins.json 未写入）");
        }
    }

    pub fn install(&self, manifest: PluginManifest, plugin_path: String) -> Result<(), String> {
        if self.plugins.contains_key(&manifest.id) {
            return Err(format!("Plugin {} already installed", manifest.id));
        }

        let plugin = InstalledPlugin {
            manifest: manifest.clone(),
            enabled: true,
            path: plugin_path,
        };

        self.plugins.insert(manifest.id.clone(), plugin);
        self.save_state();
        info!("已安装插件: {}", manifest.id);
        Ok(())
    }

    pub fn uninstall(&self, id: &str) -> Result<(), String> {
        if let Some((_, plugin)) = self.plugins.remove(id) {
            let plugin_path = PathBuf::from(&plugin.path);
            if plugin_path.exists() {
                if let Err(e) = std::fs::remove_dir_all(&plugin_path) {
                    warn!("删除插件目录失败 {:?}: {}", plugin_path, e);
                }
            }
            self.save_state();
            info!("已卸载插件: {}", id);
            Ok(())
        } else {
            Err(format!("插件 {} 未找到", id))
        }
    }

    pub fn enable(&self, id: &str) -> Result<(), String> {
        if let Some(mut plugin) = self.plugins.get_mut(id) {
            plugin.enabled = true;
            drop(plugin);
            self.save_state();
            Ok(())
        } else {
            Err(format!("插件 {} 未找到", id))
        }
    }

    pub fn disable(&self, id: &str) -> Result<(), String> {
        if let Some(mut plugin) = self.plugins.get_mut(id) {
            plugin.enabled = false;
            drop(plugin);
            self.save_state();
            Ok(())
        } else {
            Err(format!("插件 {} 未找到", id))
        }
    }

    pub fn update_config(&self, id: &str, config: serde_json::Value) -> Result<(), String> {
        if let Some(mut plugin) = self.plugins.get_mut(id) {
            plugin.manifest.config = config.clone();
            // Also save to manifest.json file
            let manifest_path = PathBuf::from(&plugin.path).join("manifest.json");
            if let Ok(content) = serde_json::to_string_pretty(&plugin.manifest) {
                if let Err(e) = std::fs::write(&manifest_path, content) {
                    warn!("写入插件 manifest 失败 {:?}: {}", manifest_path, e);
                }
            } else {
                warn!("序列化插件 manifest 失败（未写入到磁盘）: {}", id);
            }
            drop(plugin);
            self.save_state();
            info!("已更新插件 {} 配置", id);
            Ok(())
        } else {
            Err(format!("插件 {} 未找到", id))
        }
    }

    pub fn get(&self, id: &str) -> Option<InstalledPlugin> {
        self.plugins.get(id).map(|p| p.value().clone())
    }

    pub fn list(&self) -> Vec<InstalledPlugin> {
        self.plugins.iter().map(|p| p.value().clone()).collect()
    }

    pub fn list_enabled(&self) -> Vec<InstalledPlugin> {
        self.plugins
            .iter()
            .filter(|p| p.enabled)
            .map(|p| p.value().clone())
            .collect()
    }

    pub fn plugins_dir(&self) -> &PathBuf {
        &self.plugins_dir
    }
}
