use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tracing::warn;

fn default_category() -> String {
    "其他".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CommandAction {
    Help,           // 帮助指令
    Plugin(String), // 插件命令（plugin_id）
    Custom(String), // 自定义动作（插件提供）
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandParam {
    pub name: String,
    pub description: String,
    pub required: bool,
    #[serde(default)]
    pub param_type: String, // "string", "number", "user", "group"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubCommand {
    pub name: String,
    pub description: String,
    pub action: CommandAction,
    #[serde(default)]
    pub params: Vec<CommandParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub id: String,
    pub name: String, // 指令名（不含前缀）
    #[serde(default)]
    pub aliases: Vec<String>, // 别名
    #[serde(default)]
    pub pattern: Option<String>, // 自定义正则
    pub description: String,
    pub is_builtin: bool,
    pub action: CommandAction,
    #[serde(default)]
    pub subcommands: Vec<SubCommand>,
    #[serde(default)]
    pub params: Vec<CommandParam>,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default)]
    pub config: serde_json::Value, // 指令特定配置
}

pub struct CommandRegistry {
    commands: DashMap<String, Command>,
    data_path: String,
}

impl CommandRegistry {
    pub fn new(data_path: &str) -> Self {
        let registry = Self {
            commands: DashMap::new(),
            data_path: data_path.to_string(),
        };
        registry.init_builtin_commands();
        registry.load();
        registry
    }

    fn init_builtin_commands(&self) {
        let builtins = vec![Command {
            id: "help".to_string(),
            name: "帮助".to_string(),
            aliases: vec!["help".to_string(), "菜单".to_string()],
            pattern: None,
            description: "显示帮助信息，列出所有可用指令".to_string(),
            is_builtin: true,
            action: CommandAction::Help,
            subcommands: vec![],
            params: vec![CommandParam {
                name: "command".to_string(),
                description: "查看指定指令的详细帮助".to_string(),
                required: false,
                param_type: "string".to_string(),
            }],
            config: serde_json::json!({
                "mode": "text",
                "background_url": ""
            }),
            category: "核心功能".to_string(),
        }];

        for cmd in builtins {
            self.commands.insert(cmd.id.clone(), cmd);
        }
    }

    fn config_path(&self) -> String {
        format!("{}/commands.json", self.data_path)
    }

    fn load(&self) {
        let path = self.config_path();
        if Path::new(&path).exists() {
            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) => {
                    warn!("Failed to read commands.json: {:?}", e);
                    return;
                }
            };

            let saved: Vec<Command> = match serde_json::from_str::<Vec<Command>>(&content) {
                Ok(cmds) => cmds,
                Err(e) => {
                    // Backward-compat: previous versions may contain removed/unknown CommandAction variants.
                    // Parse each entry independently so one bad command won't break the whole file.
                    warn!(
                        "Failed to parse commands.json as Vec<Command> (will try best-effort per-item parse): {}",
                        e
                    );
                    let mut cmds: Vec<Command> = Vec::new();
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(arr) = val.as_array() {
                            for item in arr {
                                if let Ok(cmd) = serde_json::from_value::<Command>(item.clone()) {
                                    cmds.push(cmd);
                                }
                            }
                        }
                    }
                    cmds
                }
            };

            let mut dropped_plugin_commands = false;
            for cmd in saved {
                // Plugin commands are derived from enabled plugins at runtime.
                // They should not be persisted in commands.json, otherwise they can drift from plugin enable/disable state.
                if matches!(cmd.action, CommandAction::Plugin(_)) {
                    dropped_plugin_commands = true;
                    continue;
                }
                if cmd.is_builtin {
                    // 内置指令只更新可编辑字段
                    if let Some(mut existing) = self.commands.get_mut(&cmd.id) {
                        existing.aliases = cmd.aliases;
                        existing.pattern = cmd.pattern;
                        existing.description = cmd.description;
                        existing.config = cmd.config;
                    }
                } else {
                    self.commands.insert(cmd.id.clone(), cmd);
                }
            }

            // Best-effort migration: remove persisted plugin commands from disk.
            if dropped_plugin_commands {
                self.save();
            }
        }
    }

    pub fn save(&self) {
        let commands: Vec<Command> = self
            .commands
            .iter()
            .map(|r| r.value().clone())
            .filter(|c| !matches!(c.action, CommandAction::Plugin(_)))
            .collect();
        if let Ok(content) = serde_json::to_string_pretty(&commands) {
            let _ = fs::create_dir_all(&self.data_path);
            let _ = fs::write(self.config_path(), content);
        }
    }

    pub fn list(&self) -> Vec<Command> {
        let mut cmds: Vec<Command> = self.commands.iter().map(|r| r.value().clone()).collect();
        cmds.sort_by(|a, b| {
            // 内置指令排前面
            match (a.is_builtin, b.is_builtin) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
            }
        });
        cmds
    }

    pub fn get(&self, id: &str) -> Option<Command> {
        self.commands.get(id).map(|r| r.value().clone())
    }

    pub fn create(&self, cmd: Command) -> Result<(), String> {
        if self.commands.contains_key(&cmd.id) {
            return Err("指令ID已存在".to_string());
        }
        self.commands.insert(cmd.id.clone(), cmd);
        self.save();
        Ok(())
    }

    pub fn update(&self, id: &str, updates: serde_json::Value) -> Result<(), String> {
        // 在独立作用域内修改，确保锁在 save() 前释放
        {
            let mut cmd = self.commands.get_mut(id).ok_or("指令不存在")?;
            if matches!(cmd.action, CommandAction::Plugin(_)) {
                return Err("插件指令不可编辑，请在插件中心启用/禁用对应插件".to_string());
            }
            if cmd.is_builtin {
                if let Some(aliases) = updates.get("aliases").and_then(|v| v.as_array()) {
                    cmd.aliases = aliases
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                }
                if let Some(pattern) = updates.get("pattern") {
                    cmd.pattern = pattern.as_str().map(String::from);
                }
                if let Some(desc) = updates.get("description").and_then(|v| v.as_str()) {
                    cmd.description = desc.to_string();
                }
                if let Some(config) = updates.get("config") {
                    cmd.config = config.clone();
                }
            } else {
                if let Some(name) = updates.get("name").and_then(|v| v.as_str()) {
                    cmd.name = name.to_string();
                }
                if let Some(aliases) = updates.get("aliases").and_then(|v| v.as_array()) {
                    cmd.aliases = aliases
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                }
                if let Some(pattern) = updates.get("pattern") {
                    cmd.pattern = pattern.as_str().map(String::from);
                }
                if let Some(desc) = updates.get("description").and_then(|v| v.as_str()) {
                    cmd.description = desc.to_string();
                }
                if let Some(config) = updates.get("config") {
                    cmd.config = config.clone();
                }
            }
        } // 锁在这里释放
        self.save();
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        if let Some(cmd) = self.commands.get(id) {
            if cmd.is_builtin {
                return Err("内置指令不可删除".to_string());
            }
            if matches!(cmd.action, CommandAction::Plugin(_)) {
                return Err("插件指令不可删除，请在插件中心启用/禁用对应插件".to_string());
            }
        }
        self.commands.remove(id);
        self.save();
        Ok(())
    }

    /// 注册插件命令
    pub fn register_plugin_command(
        &self,
        plugin_id: &str,
        name: &str,
        aliases: Vec<String>,
        description: &str,
    ) {
        let cmd = Command {
            id: format!("plugin_{}_{}", plugin_id, name),
            name: name.to_string(),
            aliases,
            pattern: None,
            description: description.to_string(),
            is_builtin: false,
            action: CommandAction::Plugin(plugin_id.to_string()),
            subcommands: vec![],
            params: vec![],
            category: "插件".to_string(),
            config: serde_json::json!({}),
        };
        self.commands.insert(cmd.id.clone(), cmd);
    }

    /// 注销插件的所有命令
    pub fn unregister_plugin_commands(&self, plugin_id: &str) {
        let prefix = format!("plugin_{}_", plugin_id);
        let keys_to_remove: Vec<String> = self
            .commands
            .iter()
            .filter(|r| r.key().starts_with(&prefix))
            .map(|r| r.key().clone())
            .collect();
        for key in keys_to_remove {
            self.commands.remove(&key);
        }
    }
}
