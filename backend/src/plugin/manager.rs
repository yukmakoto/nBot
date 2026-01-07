use crate::plugin::runtime::{PluginOutput, PluginRuntime};
use crate::plugin::types::{InstalledPlugin, PluginCodeType};
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tracing::info;

/// 带有来源插件 ID 的输出
#[derive(Debug, Clone)]
pub struct PluginOutputWithSource {
    pub plugin_id: String,
    pub output: PluginOutput,
}

/// 插件钩子结果
pub struct HookResult {
    pub allow: bool,
    pub outputs: Vec<PluginOutputWithSource>,
}

/// 插件请求类型
pub enum PluginRequest {
    Load {
        plugin_id: String,
        plugin_root: String,
        entry: String,
        code_type: PluginCodeType,
        config: serde_json::Value,
        respond: oneshot::Sender<Result<(), String>>,
    },
    UpdateConfig {
        plugin_id: String,
        config: serde_json::Value,
        respond: oneshot::Sender<Result<(), String>>,
    },
    Unload {
        plugin_id: String,
        respond: oneshot::Sender<Result<(), String>>,
    },
    PreCommand {
        plugin_id: String,
        ctx: serde_json::Value,
        respond: oneshot::Sender<HookResult>,
    },
    PreMessage {
        plugin_id: String,
        ctx: serde_json::Value,
        respond: oneshot::Sender<HookResult>,
    },
    OnCommand {
        plugin_id: String,
        ctx: serde_json::Value,
        respond: oneshot::Sender<Result<Vec<PluginOutput>, String>>,
    },
    OnNotice {
        plugin_id: String,
        ctx: serde_json::Value,
        respond: oneshot::Sender<HookResult>,
    },
    OnMetaEvent {
        plugin_id: String,
        ctx: serde_json::Value,
        respond: oneshot::Sender<HookResult>,
    },
    OnLlmResponse {
        plugin_id: String,
        request_id: String,
        success: bool,
        content: String,
        respond: oneshot::Sender<Result<Vec<PluginOutput>, String>>,
    },
    OnGroupInfoResponse {
        plugin_id: String,
        request_id: String,
        info_type: String,
        success: bool,
        data: String,
        respond: oneshot::Sender<Result<Vec<PluginOutput>, String>>,
    },
}

/// 插件管理器 - 管理所有插件运行时
pub struct PluginManager {
    tx: mpsc::Sender<PluginRequest>,
    loaded_plugins: Arc<DashMap<String, ()>>,
}

impl PluginManager {
    pub fn new(data_dir: &str) -> Self {
        let (tx, rx) = mpsc::channel::<PluginRequest>(100);
        let loaded_plugins = Arc::new(DashMap::new());
        let loaded_clone = loaded_plugins.clone();
        let data_dir_clone = data_dir.to_string();

        // 在专门的线程中运行插件
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    tracing::error!("创建插件运行时失败: {}", e);
                    return;
                }
            };

            rt.block_on(async move {
                plugin_worker(rx, loaded_clone, data_dir_clone).await;
            });
        });

        Self { tx, loaded_plugins }
    }

    /// 加载插件
    pub async fn load(&self, plugin: &InstalledPlugin) -> Result<(), String> {
        let (respond, rx) = oneshot::channel();
        self.tx
            .send(PluginRequest::Load {
                plugin_id: plugin.manifest.id.clone(),
                plugin_root: plugin.path.clone(),
                entry: plugin.manifest.entry.clone(),
                code_type: plugin.manifest.code_type,
                config: plugin.manifest.config.clone(),
                respond,
            })
            .await
            .map_err(|e| format!("发送请求失败: {}", e))?;

        rx.await.map_err(|_| "接收响应失败".to_string())?
    }

    /// 卸载插件
    pub async fn unload(&self, plugin_id: &str) -> Result<(), String> {
        let (respond, rx) = oneshot::channel();
        self.tx
            .send(PluginRequest::Unload {
                plugin_id: plugin_id.to_string(),
                respond,
            })
            .await
            .map_err(|e| format!("发送请求失败: {}", e))?;

        rx.await.map_err(|_| "接收响应失败".to_string())?
    }

    /// 更新已加载插件的配置（不会重载插件）
    pub async fn update_config(
        &self,
        plugin_id: &str,
        config: serde_json::Value,
    ) -> Result<(), String> {
        let (respond, rx) = oneshot::channel();
        self.tx
            .send(PluginRequest::UpdateConfig {
                plugin_id: plugin_id.to_string(),
                config,
                respond,
            })
            .await
            .map_err(|e| format!("发送请求失败: {}", e))?;

        rx.await.map_err(|_| "接收响应失败".to_string())?
    }

    /// 调用 preCommand 钩子
    pub async fn pre_command(&self, ctx: serde_json::Value) -> HookResult {
        let plugin_ids = self.ordered_plugin_ids();

        let mut all_outputs = Vec::new();
        for plugin_id in plugin_ids {
            let (respond, rx) = oneshot::channel();
            if let Err(e) = self
                .tx
                .send(PluginRequest::PreCommand {
                    plugin_id: plugin_id.clone(),
                    ctx: ctx.clone(),
                    respond,
                })
                .await
            {
                tracing::error!("发送插件 preCommand 请求失败: {}: {}", plugin_id, e);
                return HookResult {
                    allow: false,
                    outputs: all_outputs,
                };
            }

            match rx.await {
                Ok(result) => {
                    all_outputs.extend(result.outputs);
                    if !result.allow {
                        return HookResult {
                            allow: false,
                            outputs: all_outputs,
                        };
                    }
                }
                Err(e) => {
                    tracing::error!("接收插件 preCommand 响应失败: {}: {}", plugin_id, e);
                    return HookResult {
                        allow: false,
                        outputs: all_outputs,
                    };
                }
            }
        }
        HookResult {
            allow: true,
            outputs: all_outputs,
        }
    }

    /// 调用 preMessage 钩子 - 在消息处理前调用，返回 false 则阻止处理
    pub async fn pre_message(&self, ctx: serde_json::Value) -> HookResult {
        let plugin_ids = self.ordered_plugin_ids();

        let mut all_outputs = Vec::new();
        for plugin_id in plugin_ids {
            let (respond, rx) = oneshot::channel();
            if let Err(e) = self
                .tx
                .send(PluginRequest::PreMessage {
                    plugin_id: plugin_id.clone(),
                    ctx: ctx.clone(),
                    respond,
                })
                .await
            {
                tracing::error!("发送插件 preMessage 请求失败: {}: {}", plugin_id, e);
                return HookResult {
                    allow: false,
                    outputs: all_outputs,
                };
            }

            match rx.await {
                Ok(result) => {
                    all_outputs.extend(result.outputs);
                    if !result.allow {
                        return HookResult {
                            allow: false,
                            outputs: all_outputs,
                        };
                    }
                }
                Err(e) => {
                    tracing::error!("接收插件 preMessage 响应失败: {}: {}", plugin_id, e);
                    return HookResult {
                        allow: false,
                        outputs: all_outputs,
                    };
                }
            }
        }
        HookResult {
            allow: true,
            outputs: all_outputs,
        }
    }

    /// 调用 onCommand 钩子 - 执行插件命令
    pub async fn on_command(
        &self,
        plugin_id: &str,
        ctx: serde_json::Value,
    ) -> Result<Vec<PluginOutput>, String> {
        let (respond, rx) = oneshot::channel();
        self.tx
            .send(PluginRequest::OnCommand {
                plugin_id: plugin_id.to_string(),
                ctx,
                respond,
            })
            .await
            .map_err(|e| format!("发送插件 onCommand 请求失败: {}", e))?;

        rx.await
            .map_err(|_| "接收插件 onCommand 响应失败".to_string())?
    }

    /// 调用 onNotice 钩子 - 处理通知事件（如灰条消息）
    pub async fn on_notice(&self, ctx: serde_json::Value) -> HookResult {
        let plugin_ids = self.ordered_plugin_ids();

        let mut all_outputs = Vec::new();
        for plugin_id in plugin_ids {
            let (respond, rx) = oneshot::channel();
            if let Err(e) = self
                .tx
                .send(PluginRequest::OnNotice {
                    plugin_id: plugin_id.clone(),
                    ctx: ctx.clone(),
                    respond,
                })
                .await
            {
                tracing::error!("发送插件 onNotice 请求失败: {}: {}", plugin_id, e);
                continue;
            }

            match rx.await {
                Ok(result) => {
                    all_outputs.extend(result.outputs);
                    if !result.allow {
                        return HookResult {
                            allow: false,
                            outputs: all_outputs,
                        };
                    }
                }
                Err(e) => {
                    tracing::error!("接收插件 onNotice 响应失败: {}: {}", plugin_id, e);
                }
            }
        }
        HookResult {
            allow: true,
            outputs: all_outputs,
        }
    }

    /// 调用 onMetaEvent 钩子 - 处理 meta_event（如 heartbeat）
    pub async fn on_meta_event(&self, ctx: serde_json::Value) -> HookResult {
        let plugin_ids = self.ordered_plugin_ids();

        let mut all_outputs = Vec::new();
        for plugin_id in plugin_ids {
            let (respond, rx) = oneshot::channel();
            if let Err(e) = self
                .tx
                .send(PluginRequest::OnMetaEvent {
                    plugin_id: plugin_id.clone(),
                    ctx: ctx.clone(),
                    respond,
                })
                .await
            {
                tracing::error!("发送插件 onMetaEvent 请求失败: {}: {}", plugin_id, e);
                continue;
            }

            match rx.await {
                Ok(result) => {
                    all_outputs.extend(result.outputs);
                    if !result.allow {
                        return HookResult {
                            allow: false,
                            outputs: all_outputs,
                        };
                    }
                }
                Err(e) => {
                    tracing::error!("接收插件 onMetaEvent 响应失败: {}: {}", plugin_id, e);
                }
            }
        }
        HookResult {
            allow: true,
            outputs: all_outputs,
        }
    }

    /// 调用单个插件的 onMetaEvent（用于内部 tick 等定向事件）
    pub async fn on_meta_event_for(&self, plugin_id: &str, ctx: serde_json::Value) -> HookResult {
        if !self.loaded_plugins.contains_key(plugin_id) {
            return HookResult {
                allow: true,
                outputs: Vec::new(),
            };
        }

        let (respond, rx) = oneshot::channel();
        if let Err(e) = self
            .tx
            .send(PluginRequest::OnMetaEvent {
                plugin_id: plugin_id.to_string(),
                ctx,
                respond,
            })
            .await
        {
            tracing::error!("发送插件 onMetaEvent 请求失败: {}: {}", plugin_id, e);
            return HookResult {
                allow: true,
                outputs: Vec::new(),
            };
        }

        match rx.await {
            Ok(result) => result,
            Err(e) => {
                tracing::error!("接收插件 onMetaEvent 响应失败: {}: {}", plugin_id, e);
                HookResult {
                    allow: true,
                    outputs: Vec::new(),
                }
            }
        }
    }

    fn ordered_plugin_ids(&self) -> Vec<String> {
        let mut plugin_ids: Vec<String> = self
            .loaded_plugins
            .iter()
            .map(|r| r.key().clone())
            .collect();

        plugin_ids.sort_by(|a, b| {
            let pa = plugin_priority(a);
            let pb = plugin_priority(b);
            pa.cmp(&pb).then_with(|| a.cmp(b))
        });

        plugin_ids
    }

    /// 调用 onLlmResponse 钩子 - LLM 调用完成后的回调
    pub async fn on_llm_response(
        &self,
        plugin_id: &str,
        request_id: &str,
        success: bool,
        content: &str,
    ) -> Result<Vec<PluginOutput>, String> {
        let (respond, rx) = oneshot::channel();
        self.tx
            .send(PluginRequest::OnLlmResponse {
                plugin_id: plugin_id.to_string(),
                request_id: request_id.to_string(),
                success,
                content: content.to_string(),
                respond,
            })
            .await
            .map_err(|e| format!("发送插件 onLlmResponse 请求失败: {}", e))?;

        rx.await
            .map_err(|_| "接收插件 onLlmResponse 响应失败".to_string())?
    }

    /// 调用 onGroupInfoResponse 钩子 - 群信息获取完成后的回调
    pub async fn on_group_info_response(
        &self,
        plugin_id: &str,
        request_id: &str,
        info_type: &str,
        success: bool,
        data: &str,
    ) -> Result<Vec<PluginOutput>, String> {
        let (respond, rx) = oneshot::channel();
        self.tx
            .send(PluginRequest::OnGroupInfoResponse {
                plugin_id: plugin_id.to_string(),
                request_id: request_id.to_string(),
                info_type: info_type.to_string(),
                success,
                data: data.to_string(),
                respond,
            })
            .await
            .map_err(|e| format!("发送插件 onGroupInfoResponse 请求失败: {}", e))?;

        rx.await
            .map_err(|_| "接收插件 onGroupInfoResponse 响应失败".to_string())?
    }

    /// 检查插件是否已加载
    pub fn is_loaded(&self, plugin_id: &str) -> bool {
        self.loaded_plugins.contains_key(plugin_id)
    }
}

fn plugin_priority(plugin_id: &str) -> i32 {
    // Lower means earlier. Keep access-control plugins first to prevent side effects.
    match plugin_id {
        "whitelist" => -100,
        _ => 0,
    }
}

#[derive(Clone)]
struct LoadedPluginMeta {
    plugin_id: String,
    plugin_root: String,
    entry: String,
    code_type: PluginCodeType,
    config: serde_json::Value,
}

struct LoadedPluginRuntime {
    meta: LoadedPluginMeta,
    runtime: PluginRuntime,
}

/// 插件工作线程
async fn plugin_worker(
    mut rx: mpsc::Receiver<PluginRequest>,
    loaded: Arc<DashMap<String, ()>>,
    data_dir: String,
) {
    let mut runtimes: std::collections::HashMap<String, LoadedPluginRuntime> =
        std::collections::HashMap::new();
    // NOTE: V8 requires isolates to be dropped in reverse order of creation.
    // We keep a creation-order stack and perform LIFO unload/reload to avoid aborts when disabling plugins.
    let mut load_stack: Vec<String> = Vec::new();

    async fn load_one(
        runtimes: &mut std::collections::HashMap<String, LoadedPluginRuntime>,
        load_stack: &mut Vec<String>,
        loaded: &DashMap<String, ()>,
        data_dir: &str,
        meta: LoadedPluginMeta,
    ) -> Result<(), String> {
        if runtimes.contains_key(&meta.plugin_id) {
            return Ok(());
        }
        let plugin_id = meta.plugin_id.clone();
        let mut runtime =
            PluginRuntime::new(&plugin_id, meta.config.clone(), data_dir, &meta.plugin_root)?;
        runtime.load_plugin(&meta.entry, meta.code_type).await?;
        runtimes.insert(
            plugin_id.clone(),
            LoadedPluginRuntime {
                meta,
                runtime,
            },
        );
        loaded.insert(plugin_id.clone(), ());
        load_stack.push(plugin_id.clone());
        info!("插件 {} 已加载", plugin_id);
        Ok(())
    }

    async fn unload_last(
        runtimes: &mut std::collections::HashMap<String, LoadedPluginRuntime>,
        load_stack: &mut Vec<String>,
        loaded: &DashMap<String, ()>,
        plugin_id: &str,
    ) -> Result<LoadedPluginMeta, String> {
        let last = load_stack
            .last()
            .map(|s| s.as_str())
            .ok_or_else(|| "插件栈为空".to_string())?;
        if last != plugin_id {
            return Err(format!(
                "插件 {} 不是最后加载的插件（last={}）",
                plugin_id, last
            ));
        }
        load_stack.pop();
        if let Some(mut entry) = runtimes.remove(plugin_id) {
            if let Err(e) = entry.runtime.on_disable().await {
                tracing::warn!("插件 {} onDisable 失败: {}", plugin_id, e);
            }
            loaded.remove(plugin_id);
            info!("插件 {} 已卸载", plugin_id);
            Ok(entry.meta)
        } else {
            Err(format!("插件 {} 未加载", plugin_id))
        }
    }

    while let Some(req) = rx.recv().await {
        match req {
            PluginRequest::Load {
                plugin_id,
                plugin_root,
                entry,
                code_type,
                config,
                respond,
            } => {
                let meta = LoadedPluginMeta {
                    plugin_id,
                    plugin_root,
                    entry,
                    code_type,
                    config,
                };
                let result = load_one(
                    &mut runtimes,
                    &mut load_stack,
                    &loaded,
                    &data_dir,
                    meta,
                )
                .await;
                let _ = respond.send(result);
            }
            PluginRequest::UpdateConfig {
                plugin_id,
                config,
                respond,
            } => {
                let result = async {
                    if let Some(entry) = runtimes.get_mut(&plugin_id) {
                        entry.runtime.update_config(config.clone()).await?;
                        entry.meta.config = config;
                        Ok(())
                    } else {
                        Err(format!("插件 {} 未加载", plugin_id))
                    }
                }
                .await;
                let _ = respond.send(result);
            }
            PluginRequest::Unload { plugin_id, respond } => {
                let result = async {
                    if !runtimes.contains_key(&plugin_id) {
                        return Err(format!("插件 {} 未加载", plugin_id));
                    }

                    // Enforce reverse-drop order required by V8 isolates.
                    // If unloading a non-top plugin, temporarily unload plugins above it (LIFO),
                    // unload the target, then reload the temporarily-unloaded plugins.
                    let pos = match load_stack.iter().position(|id| id == &plugin_id) {
                        Some(p) => p,
                        None => {
                            // Fallback: treat as loaded but not tracked; try direct unload (best-effort).
                            if let Some(mut entry) = runtimes.remove(&plugin_id) {
                                if let Err(e) = entry.runtime.on_disable().await {
                                    tracing::warn!("插件 {} onDisable 失败: {}", plugin_id, e);
                                }
                                loaded.remove(&plugin_id);
                                info!("插件 {} 已卸载", plugin_id);
                                return Ok(());
                            }
                            return Err(format!("插件 {} 未加载", plugin_id));
                        }
                    };

                    let above_ids: Vec<String> = load_stack
                        .iter()
                        .skip(pos + 1)
                        .cloned()
                        .collect();
                    let mut reload_metas: Vec<LoadedPluginMeta> = Vec::new();
                    for id in &above_ids {
                        if let Some(entry) = runtimes.get(id) {
                            reload_metas.push(entry.meta.clone());
                        }
                    }

                    for id in above_ids.iter().rev() {
                        // Best-effort: if this fails, continue to avoid leaving the process in a bad state.
                        let _ = unload_last(&mut runtimes, &mut load_stack, &loaded, id).await;
                    }

                    // Now the target should be the top of the stack.
                    let _ = unload_last(&mut runtimes, &mut load_stack, &loaded, &plugin_id).await;

                    for meta in reload_metas {
                        if let Err(e) = load_one(
                            &mut runtimes,
                            &mut load_stack,
                            &loaded,
                            &data_dir,
                            meta.clone(),
                        )
                        .await
                        {
                            tracing::error!(
                                "插件 {} 重新加载失败（已暂时卸载，需手动重启恢复）: {}",
                                meta.plugin_id,
                                e
                            );
                        }
                    }

                    Ok(())
                }
                .await;
                let _ = respond.send(result);
            }
            PluginRequest::PreCommand {
                plugin_id,
                ctx,
                respond,
            } => {
                let result = if let Some(entry) = runtimes.get_mut(&plugin_id) {
                    match entry.runtime.pre_command(&ctx).await {
                        Ok((allow, outputs)) => HookResult {
                            allow,
                            outputs: outputs
                                .into_iter()
                                .map(|o| PluginOutputWithSource {
                                    plugin_id: plugin_id.clone(),
                                    output: o,
                                })
                                .collect(),
                        },
                        Err(e) => {
                            tracing::error!("插件 {} preCommand 失败: {}", plugin_id, e);
                            HookResult {
                                allow: false,
                                outputs: Vec::new(),
                            }
                        }
                    }
                } else {
                    HookResult {
                        allow: false,
                        outputs: Vec::new(),
                    }
                };
                let _ = respond.send(result);
            }
            PluginRequest::PreMessage {
                plugin_id,
                ctx,
                respond,
            } => {
                let result = if let Some(entry) = runtimes.get_mut(&plugin_id) {
                    match entry.runtime.pre_message(&ctx).await {
                        Ok((allow, outputs)) => HookResult {
                            allow,
                            outputs: outputs
                                .into_iter()
                                .map(|o| PluginOutputWithSource {
                                    plugin_id: plugin_id.clone(),
                                    output: o,
                                })
                                .collect(),
                        },
                        Err(e) => {
                            tracing::error!("插件 {} preMessage 失败: {}", plugin_id, e);
                            HookResult {
                                allow: false,
                                outputs: Vec::new(),
                            }
                        }
                    }
                } else {
                    HookResult {
                        allow: false,
                        outputs: Vec::new(),
                    }
                };
                let _ = respond.send(result);
            }
            PluginRequest::OnCommand {
                plugin_id,
                ctx,
                respond,
            } => {
                let result = if let Some(entry) = runtimes.get_mut(&plugin_id) {
                    entry.runtime.on_command(&ctx).await
                } else {
                    Err(format!("插件 {} 未加载", plugin_id))
                };
                let _ = respond.send(result);
            }
            PluginRequest::OnNotice {
                plugin_id,
                ctx,
                respond,
            } => {
                let result = if let Some(entry) = runtimes.get_mut(&plugin_id) {
                    match entry.runtime.on_notice(&ctx).await {
                        Ok((allow, outputs)) => HookResult {
                            allow,
                            outputs: outputs
                                .into_iter()
                                .map(|o| PluginOutputWithSource {
                                    plugin_id: plugin_id.clone(),
                                    output: o,
                                })
                                .collect(),
                        },
                        Err(e) => {
                            tracing::error!("插件 {} onNotice 失败: {}", plugin_id, e);
                            HookResult {
                                allow: true,
                                outputs: Vec::new(),
                            }
                        }
                    }
                } else {
                    HookResult {
                        allow: true,
                        outputs: Vec::new(),
                    }
                };
                let _ = respond.send(result);
            }
            PluginRequest::OnMetaEvent {
                plugin_id,
                ctx,
                respond,
            } => {
                let result = if let Some(entry) = runtimes.get_mut(&plugin_id) {
                    match entry.runtime.on_meta_event(&ctx).await {
                        Ok((allow, outputs)) => HookResult {
                            allow,
                            outputs: outputs
                                .into_iter()
                                .map(|o| PluginOutputWithSource {
                                    plugin_id: plugin_id.clone(),
                                    output: o,
                                })
                                .collect(),
                        },
                        Err(e) => {
                            tracing::error!("插件 {} onMetaEvent 失败: {}", plugin_id, e);
                            HookResult {
                                allow: true,
                                outputs: Vec::new(),
                            }
                        }
                    }
                } else {
                    HookResult {
                        allow: true,
                        outputs: Vec::new(),
                    }
                };
                let _ = respond.send(result);
            }
            PluginRequest::OnLlmResponse {
                plugin_id,
                request_id,
                success,
                content,
                respond,
            } => {
                let result = if let Some(entry) = runtimes.get_mut(&plugin_id) {
                    entry.runtime
                        .on_llm_response(&request_id, success, &content)
                        .await
                } else {
                    Err(format!("插件 {} 未加载", plugin_id))
                };
                let _ = respond.send(result);
            }
            PluginRequest::OnGroupInfoResponse {
                plugin_id,
                request_id,
                info_type,
                success,
                data,
                respond,
            } => {
                let result = if let Some(entry) = runtimes.get_mut(&plugin_id) {
                    entry.runtime
                        .on_group_info_response(&request_id, &info_type, success, &data)
                        .await
                } else {
                    Err(format!("插件 {} 未加载", plugin_id))
                };
                let _ = respond.send(result);
            }
        }
    }

    // Channel closed: best-effort unload in reverse creation order to satisfy V8 isolate drop rules.
    while let Some(id) = load_stack.last().cloned() {
        let _ = unload_last(&mut runtimes, &mut load_stack, &loaded, &id).await;
    }
}
