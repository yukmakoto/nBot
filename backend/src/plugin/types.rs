use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PluginType {
    Bot,
    Platform,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PluginCodeType {
    /// Legacy single-file plugin loaded via `execute_script` wrapper (top-level `return { ... }` supported).
    Script,
    /// ES module entry; allows `import` and multi-file directory layouts.
    Module,
}

fn default_plugin_entry() -> String {
    "index.js".to_string()
}

fn default_plugin_code_type() -> PluginCodeType {
    PluginCodeType::Script
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSchemaItem {
    pub key: String,
    #[serde(rename = "type")]
    pub field_type: String, // "string", "number", "boolean", "select", "array"
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    #[serde(default)]
    pub options: Option<Vec<ConfigSelectOption>>, // for select type
    #[serde(default)]
    pub item_type: Option<String>, // for array type
    #[serde(default)]
    pub min: Option<f64>, // for number type
    #[serde(default)]
    pub max: Option<f64>, // for number type
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSelectOption {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    #[serde(rename = "type")]
    pub plugin_type: PluginType,
    /// Plugin entry file path relative to the plugin directory.
    /// For directory mode, it may point to a folder (we will use `<entry>/index.js`).
    #[serde(default = "default_plugin_entry")]
    pub entry: String,
    /// How to load the entry. `script` is backward-compatible; `module` enables multi-file imports.
    #[serde(default = "default_plugin_code_type")]
    pub code_type: PluginCodeType,
    #[serde(default)]
    pub permissions: Vec<String>,
    pub signature: Option<String>,
    #[serde(default)]
    pub builtin: bool,
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default)]
    pub config_schema: Vec<ConfigSchemaItem>,
    #[serde(default)]
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPlugin {
    pub manifest: PluginManifest,
    pub enabled: bool,
    pub path: String,
}
