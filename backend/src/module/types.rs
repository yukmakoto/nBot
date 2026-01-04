use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BotModule {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(rename = "is_system", default)]
    pub builtin: bool,
    #[serde(default)]
    pub config: serde_json::Value,
}

pub struct ModuleRegistry {
    modules: DashMap<String, BotModule>,
    data_path: String,
}

impl ModuleRegistry {
    pub fn new(data_path: &str) -> Self {
        let registry = Self {
            modules: DashMap::new(),
            data_path: data_path.to_string(),
        };
        registry.init_default_modules();
        registry.load_config();
        registry
    }

    fn init_default_modules(&self) {
        let defaults = vec![
            BotModule {
                id: "llm".to_string(),
                name: "LLM 大语言模型".to_string(),
                description: "提供 LLM 服务供其他模块/插件调用".to_string(),
                icon: "brain".to_string(),
                enabled: false,
                builtin: true,
                config: serde_json::json!({
                    "providers": [],
                    "model_library": [],
                    "models": {},
                    "default_model": "default",
                    "tavily_api_key": ""
                }),
            },
            BotModule {
                id: "admin".to_string(),
                name: "管理员模块".to_string(),
                description: "设置机器人管理员，管理员可执行特权指令".to_string(),
                icon: "users".to_string(),
                enabled: false,
                builtin: true,
                config: serde_json::json!({
                    "admins": [],
                    "super_admins": []
                }),
            },
            BotModule {
                id: "command".to_string(),
                name: "指令模块".to_string(),
                description: "自定义指令前缀和指令解析规则".to_string(),
                icon: "terminal".to_string(),
                enabled: true,
                builtin: true,
                config: serde_json::json!({
                    "prefix": "/",
                    "aliases": {}
                }),
            },
        ];

        for module in defaults {
            self.modules.insert(module.id.clone(), module);
        }
    }

    fn config_path(&self) -> String {
        format!("{}/modules.json", self.data_path)
    }

    fn load_config(&self) {
        let path = self.config_path();
        if Path::new(&path).exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(saved) = serde_json::from_str::<Vec<BotModule>>(&content) {
                    for module in saved {
                        if let Some(mut existing) = self.modules.get_mut(&module.id) {
                            existing.enabled = module.enabled;
                            existing.config = module.config;
                        }
                    }
                }
            }
        }
    }

    pub fn save_config(&self) {
        let modules: Vec<BotModule> = self.modules.iter().map(|r| r.value().clone()).collect();
        if let Ok(content) = serde_json::to_string_pretty(&modules) {
            let _ = fs::create_dir_all(&self.data_path);
            let _ = fs::write(self.config_path(), content);
        }
    }

    pub fn list(&self) -> Vec<BotModule> {
        self.modules.iter().map(|r| r.value().clone()).collect()
    }

    pub fn get(&self, id: &str) -> Option<BotModule> {
        self.modules.get(id).map(|r| r.value().clone())
    }

    pub fn enable(&self, id: &str) -> Result<(), String> {
        {
            if let Some(mut module) = self.modules.get_mut(id) {
                module.enabled = true;
            } else {
                return Err("Module not found".to_string());
            }
        } // 锁在这里释放
        self.save_config();
        Ok(())
    }

    pub fn disable(&self, id: &str) -> Result<(), String> {
        {
            if let Some(mut module) = self.modules.get_mut(id) {
                module.enabled = false;
            } else {
                return Err("Module not found".to_string());
            }
        } // 锁在这里释放
        self.save_config();
        Ok(())
    }

    pub fn update_config(&self, id: &str, config: serde_json::Value) -> Result<(), String> {
        {
            if let Some(mut module) = self.modules.get_mut(id) {
                module.config = config;
            } else {
                return Err("Module not found".to_string());
            }
        } // 锁在这里释放
        self.save_config();
        Ok(())
    }
}
