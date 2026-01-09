use deno_core::{extension, JsRuntime, RuntimeOptions};
use std::path::PathBuf;
use std::rc::Rc;
use tracing::debug;

mod ops;
mod state;

use ops::*;
use state::{get_hook_result, reset_hook_state, take_outputs, PluginOpState};

pub use state::{ForwardNode, MediaBundleItem, PluginOutput};

use super::types::PluginCodeType;

extension!(
    nbot_plugin,
    ops = [op_send_message, op_send_reply, op_call_api, op_log, op_set_hook_result, op_now, op_get_config, op_set_config, op_storage_set, op_storage_get, op_storage_delete, op_get_plugin_id, op_call_llm_forward, op_call_llm_forward_from_url, op_call_llm_forward_archive_from_url, op_call_llm_forward_image_from_url, op_call_llm_forward_video_from_url, op_call_llm_forward_audio_from_url, op_call_llm_forward_media_bundle, op_call_llm_chat, op_call_llm_chat_with_search, op_send_forward_message, op_http_fetch, op_render_markdown_image, op_render_html_image, op_fetch_group_notice, op_fetch_group_msg_history, op_fetch_group_files, op_fetch_group_file_url, op_fetch_friend_list, op_fetch_group_list, op_fetch_group_member_list, op_download_file],
    esm_entry_point = "ext:nbot_plugin/runtime.js",
    esm = [dir "src/plugin/js", "runtime.js"],
);

pub struct PluginRuntime {
    runtime: JsRuntime,
    plugin_id: String,
    plugin_root: PathBuf,
}

impl PluginRuntime {
    pub fn new(
        plugin_id: &str,
        config: serde_json::Value,
        data_dir: &str,
        plugin_root: &str,
    ) -> Result<Self, String> {
        let mut runtime = JsRuntime::new(RuntimeOptions {
            extensions: vec![nbot_plugin::init_ops_and_esm()],
            module_loader: Some(Rc::new(deno_core::FsModuleLoader)),
            ..Default::default()
        });

        {
            let op_state = runtime.op_state();
            let mut op_state = op_state.borrow_mut();
            op_state.put(PluginOpState {
                plugin_id: plugin_id.to_string(),
                config,
                data_dir: data_dir.to_string(),
                hook_result: None,
                outputs: Vec::new(),
            });
        }

        Ok(Self {
            runtime,
            plugin_id: plugin_id.to_string(),
            plugin_root: PathBuf::from(plugin_root),
        })
    }

    fn resolve_entry_path(&self, entry: &str) -> Result<PathBuf, String> {
        let raw = entry.trim();
        if raw.is_empty() {
            return Err("Plugin entry is empty".to_string());
        }

        let mut path = self.plugin_root.join(raw);
        if path.is_dir() {
            path = path.join("index.js");
        }
        if !path.exists() {
            return Err(format!(
                "Plugin entry not found: {} (root: {})",
                path.to_string_lossy(),
                self.plugin_root.to_string_lossy()
            ));
        }
        Ok(path)
    }

    pub async fn load_plugin(&mut self, entry: &str, code_type: PluginCodeType) -> Result<(), String> {
        match code_type {
            PluginCodeType::Script => {
                let entry_path = self.resolve_entry_path(entry)?;
                let code = std::fs::read_to_string(&entry_path)
                    .map_err(|e| format!("读取插件入口失败 {:?}: {}", entry_path, e))?;

                let wrapped_code = format!(
                    r#"
                    const plugin = (function() {{
                        {code}
                    }})();
                    globalThis.__plugin = plugin.default || plugin;
                    "#,
                    code = code
                );

                self.runtime
                    .execute_script("<plugin>", wrapped_code)
                    .map_err(|e| format!("Failed to load plugin: {}", e))?;
            }
            PluginCodeType::Module => {
                let entry_path = self.resolve_entry_path(entry)?;
                let spec = deno_core::ModuleSpecifier::from_file_path(&entry_path)
                    .map_err(|_| format!("Invalid entry path: {}", entry_path.to_string_lossy()))?;

                let bootstrap = format!(
                    r#"
                    (async () => {{
                        const m = await import("{url}");
                        globalThis.__plugin = m.default || m;
                    }})()
                    "#,
                    url = spec.as_str()
                );

                self.runtime
                    .execute_script("<plugin_module>", bootstrap)
                    .map_err(|e| format!("Failed to load plugin module: {}", e))?;

                self.runtime
                    .run_event_loop(Default::default())
                    .await
                    .map_err(|e| format!("plugin module event loop failed: {}", e))?;
            }
        }

        // Call onEnable if exists
        let enable_code = r#"
            (async () => {
                if (globalThis.__plugin && globalThis.__plugin.onEnable) {
                    await globalThis.__plugin.onEnable();
                }
            })()
        "#;

        self.runtime
            .execute_script("<enable>", enable_code.to_string())
            .map_err(|e| format!("onEnable failed: {}", e))?;

        self.runtime
            .run_event_loop(Default::default())
            .await
            .map_err(|e| format!("onEnable event loop failed: {}", e))?;

        debug!("插件 {} 已加载", self.plugin_id);
        Ok(())
    }

    pub async fn on_disable(&mut self) -> Result<(), String> {
        let code = r#"
            (async () => {
                if (globalThis.__plugin && globalThis.__plugin.onDisable) {
                    await globalThis.__plugin.onDisable();
                }
            })()
        "#;

        self.runtime
            .execute_script("<disable>", code.to_string())
            .map_err(|e| format!("onDisable failed: {}", e))?;

        self.runtime
            .run_event_loop(Default::default())
            .await
            .map_err(|e| format!("onDisable event loop failed: {}", e))?;
        Ok(())
    }

    /// 更新插件配置，并在插件实现时触发 onConfigUpdated(newConfig)
    pub async fn update_config(&mut self, config: serde_json::Value) -> Result<(), String> {
        {
            let op_state = self.runtime.op_state();
            let mut op_state = op_state.borrow_mut();
            let state = op_state.borrow_mut::<PluginOpState>();
            state.config = config.clone();
        }

        let config_json = serde_json::to_string(&config).unwrap_or_else(|_| "{}".to_string());
        let code = format!(
            r#"
            (async () => {{
                if (!globalThis.__plugin) return;
                if (typeof globalThis.__plugin.onConfigUpdated === "function") {{
                    await globalThis.__plugin.onConfigUpdated({});
                    return;
                }}
                // Backward-compat for older plugins.
                if (typeof globalThis.__plugin.updateConfig === "function") {{
                    await globalThis.__plugin.updateConfig({});
                }}
            }})()
            "#,
            config_json, config_json
        );

        self.runtime
            .execute_script("<configUpdated>", code)
            .map_err(|e| format!("onConfigUpdated failed: {}", e))?;

        self.runtime
            .run_event_loop(Default::default())
            .await
            .map_err(|e| format!("onConfigUpdated event loop failed: {}", e))?;
        Ok(())
    }

    /// preCommand 钩子：在命令执行前调用，返回 false 则阻止执行
    pub async fn pre_command(
        &mut self,
        ctx: &serde_json::Value,
    ) -> Result<(bool, Vec<PluginOutput>), String> {
        reset_hook_state(&mut self.runtime);

        let ctx_json =
            serde_json::to_string(ctx).map_err(|e| format!("Serialize ctx failed: {e}"))?;
        let code = format!(
            r#"
            (async () => {{
                if (globalThis.__plugin && globalThis.__plugin.preCommand) {{
                    const result = await globalThis.__plugin.preCommand({});
                    Deno.core.ops.op_set_hook_result(result !== false);
                }} else {{
                    Deno.core.ops.op_set_hook_result(true);
                }}
            }})()
            "#,
            ctx_json
        );

        self.runtime
            .execute_script("<preCommand>", code)
            .map_err(|e| format!("preCommand failed: {}", e))?;

        self.runtime
            .run_event_loop(Default::default())
            .await
            .map_err(|e| format!("preCommand event loop failed: {}", e))?;

        // 获取返回值和输出
        let result = get_hook_result(&mut self.runtime);
        let outputs = take_outputs(&mut self.runtime);
        Ok((result, outputs))
    }

    /// preMessage 钩子：在消息处理前调用，返回 false 则阻止处理
    pub async fn pre_message(
        &mut self,
        ctx: &serde_json::Value,
    ) -> Result<(bool, Vec<PluginOutput>), String> {
        reset_hook_state(&mut self.runtime);

        let ctx_json =
            serde_json::to_string(ctx).map_err(|e| format!("Serialize ctx failed: {e}"))?;
        let code = format!(
            r#"
            (async () => {{
                if (globalThis.__plugin && globalThis.__plugin.preMessage) {{
                    const result = await globalThis.__plugin.preMessage({});
                    Deno.core.ops.op_set_hook_result(result !== false);
                }} else {{
                    Deno.core.ops.op_set_hook_result(true);
                }}
            }})()
            "#,
            ctx_json
        );

        self.runtime
            .execute_script("<preMessage>", code)
            .map_err(|e| format!("preMessage failed: {}", e))?;

        self.runtime
            .run_event_loop(Default::default())
            .await
            .map_err(|e| format!("preMessage event loop failed: {}", e))?;

        let result = get_hook_result(&mut self.runtime);
        let outputs = take_outputs(&mut self.runtime);
        Ok((result, outputs))
    }

    /// onCommand 钩子：执行插件命令
    pub async fn on_command(
        &mut self,
        ctx: &serde_json::Value,
    ) -> Result<Vec<PluginOutput>, String> {
        take_outputs(&mut self.runtime);

        let ctx_json =
            serde_json::to_string(ctx).map_err(|e| format!("Serialize ctx failed: {e}"))?;
        let code = format!(
            r#"
            (async () => {{
                if (globalThis.__plugin && globalThis.__plugin.onCommand) {{
                    await globalThis.__plugin.onCommand({});
                }}
            }})()
            "#,
            ctx_json
        );

        self.runtime
            .execute_script("<onCommand>", code)
            .map_err(|e| format!("onCommand failed: {}", e))?;

        self.runtime
            .run_event_loop(Default::default())
            .await
            .map_err(|e| format!("onCommand event loop failed: {}", e))?;

        Ok(take_outputs(&mut self.runtime))
    }

    /// onNotice 钩子：处理通知事件（如灰条消息）
    pub async fn on_notice(
        &mut self,
        ctx: &serde_json::Value,
    ) -> Result<(bool, Vec<PluginOutput>), String> {
        reset_hook_state(&mut self.runtime);

        let ctx_json =
            serde_json::to_string(ctx).map_err(|e| format!("Serialize ctx failed: {e}"))?;
        let code = format!(
            r#"
            (async () => {{
                if (globalThis.__plugin && globalThis.__plugin.onNotice) {{
                    const result = await globalThis.__plugin.onNotice({});
                    Deno.core.ops.op_set_hook_result(result !== false);
                }} else {{
                    Deno.core.ops.op_set_hook_result(true);
                }}
            }})()
            "#,
            ctx_json
        );

        self.runtime
            .execute_script("<onNotice>", code)
            .map_err(|e| format!("onNotice failed: {}", e))?;

        self.runtime
            .run_event_loop(Default::default())
            .await
            .map_err(|e| format!("onNotice event loop failed: {}", e))?;

        let result = get_hook_result(&mut self.runtime);
        let outputs = take_outputs(&mut self.runtime);
        Ok((result, outputs))
    }

    /// onMetaEvent 钩子：处理 meta_event（如 heartbeat）
    pub async fn on_meta_event(
        &mut self,
        ctx: &serde_json::Value,
    ) -> Result<(bool, Vec<PluginOutput>), String> {
        reset_hook_state(&mut self.runtime);

        let ctx_json =
            serde_json::to_string(ctx).map_err(|e| format!("Serialize ctx failed: {e}"))?;
        let code = format!(
            r#"
            (async () => {{
                if (globalThis.__plugin && globalThis.__plugin.onMetaEvent) {{
                    const result = await globalThis.__plugin.onMetaEvent({});
                    Deno.core.ops.op_set_hook_result(result !== false);
                }} else {{
                    Deno.core.ops.op_set_hook_result(true);
                }}
            }})()
            "#,
            ctx_json
        );

        self.runtime
            .execute_script("<onMetaEvent>", code)
            .map_err(|e| format!("onMetaEvent failed: {}", e))?;

        self.runtime
            .run_event_loop(Default::default())
            .await
            .map_err(|e| format!("onMetaEvent event loop failed: {}", e))?;

        let result = get_hook_result(&mut self.runtime);
        let outputs = take_outputs(&mut self.runtime);
        Ok((result, outputs))
    }

    /// onLlmResponse 钩子：LLM 调用完成后的回调
    /// request_id: 请求 ID（与 callLlmChat 时传入的一致）
    /// success: 是否成功
    /// content: 成功时为 LLM 回复内容，失败时为错误信息
    pub async fn on_llm_response(
        &mut self,
        request_id: &str,
        success: bool,
        content: &str,
    ) -> Result<Vec<PluginOutput>, String> {
        take_outputs(&mut self.runtime);

        let request_id_json = serde_json::to_string(request_id)
            .map_err(|e| format!("Serialize request_id failed: {e}"))?;
        let content_json =
            serde_json::to_string(content).map_err(|e| format!("Serialize content failed: {e}"))?;

        let code = format!(
            r#"
            (async () => {{
                if (globalThis.__plugin && globalThis.__plugin.onLlmResponse) {{
                    await globalThis.__plugin.onLlmResponse({{
                        requestId: {},
                        success: {},
                        content: {}
                    }});
                }}
            }})()
            "#,
            request_id_json, success, content_json
        );

        self.runtime
            .execute_script("<onLlmResponse>", code)
            .map_err(|e| format!("onLlmResponse failed: {}", e))?;

        self.runtime
            .run_event_loop(Default::default())
            .await
            .map_err(|e| format!("onLlmResponse event loop failed: {}", e))?;

        Ok(take_outputs(&mut self.runtime))
    }

    /// onGroupInfoResponse hook: callback after group info fetch completes
    /// request_id: request ID (matches the one passed to fetchGroupNotice/fetchGroupMsgHistory/etc.)
    /// info_type: type of info ("notice", "msg_history", "files", "file_url", "download")
    /// success: whether the request succeeded
    /// data: JSON string of the response data (or error message if failed)
    pub async fn on_group_info_response(
        &mut self,
        request_id: &str,
        info_type: &str,
        success: bool,
        data: &str,
    ) -> Result<Vec<PluginOutput>, String> {
        take_outputs(&mut self.runtime);

        let request_id_json = serde_json::to_string(request_id)
            .map_err(|e| format!("Serialize request_id failed: {e}"))?;
        let info_type_json = serde_json::to_string(info_type)
            .map_err(|e| format!("Serialize info_type failed: {e}"))?;

        let code = format!(
            r#"
            (async () => {{
                if (globalThis.__plugin && globalThis.__plugin.onGroupInfoResponse) {{
                    let parsedData = null;
                    try {{
                        parsedData = JSON.parse({data});
                    }} catch (e) {{
                        parsedData = {data};
                    }}
                    await globalThis.__plugin.onGroupInfoResponse({{
                        requestId: {request_id},
                        infoType: {info_type},
                        success: {success},
                        data: parsedData
                    }});
                }}
            }})()
            "#,
            request_id = request_id_json,
            info_type = info_type_json,
            success = success,
            data = serde_json::to_string(data).unwrap_or_else(|_| "null".to_string())
        );

        self.runtime
            .execute_script("<onGroupInfoResponse>", code)
            .map_err(|e| format!("onGroupInfoResponse failed: {}", e))?;

        self.runtime
            .run_event_loop(Default::default())
            .await
            .map_err(|e| format!("onGroupInfoResponse event loop failed: {}", e))?;

        Ok(take_outputs(&mut self.runtime))
    }
}
