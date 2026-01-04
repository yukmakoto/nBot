use serde::{Deserialize, Serialize};

/// 工具容器信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolContainer {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub kind: String,
    pub container_name: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip)]
    pub container_id: Option<String>,
}

impl Default for ToolContainer {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            description: String::new(),
            kind: "tool".to_string(),
            container_name: String::new(),
            status: "notfound".to_string(),
            ports: Vec::new(),
            detail: None,
            container_id: None,
        }
    }
}

fn parse_label_bool(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::String(s) => {
            let s = s.trim();
            s.eq_ignore_ascii_case("true") || s == "1" || s.eq_ignore_ascii_case("yes")
        }
        serde_json::Value::Number(n) => n.as_i64() == Some(1),
        _ => false,
    }
}

fn parse_published_ports(service: &serde_json::Value) -> Vec<u16> {
    let mut out = Vec::<u16>::new();
    let Some(ports) = service.get("ports").and_then(|v| v.as_array()) else {
        return out;
    };

    for p in ports {
        let port = match p.get("published") {
            Some(v) => {
                if let Some(pub_str) = v.as_str() {
                    pub_str.trim().parse::<u16>().ok()
                } else if let Some(n) = v.as_u64() {
                    u16::try_from(n).ok()
                } else {
                    None
                }
            }
            None => None,
        };

        if let Some(port) = port {
            out.push(port);
        }
    }

    out.sort_unstable();
    out.dedup();
    out
}

pub fn tool_from_compose_service(
    service_id: &str,
    service: &serde_json::Value,
) -> Option<ToolContainer> {
    let labels = service.get("labels")?.as_object()?;
    let is_tool = labels
        .get("nbot.tool")
        .map(parse_label_bool)
        .unwrap_or(false);
    if !is_tool {
        return None;
    }

    let name = labels
        .get("nbot.tool.name")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(service_id)
        .to_string();

    let description = labels
        .get("nbot.tool.description")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    Some(ToolContainer {
        id: service_id.to_string(),
        name,
        description,
        kind: "tool".to_string(),
        container_name: service_id.to_string(),
        status: "notfound".to_string(),
        ports: parse_published_ports(service),
        detail: None,
        container_id: None,
    })
}

pub fn infra_from_compose_service(
    service_id: &str,
    service: &serde_json::Value,
) -> Option<ToolContainer> {
    let labels = service.get("labels")?.as_object()?;
    let is_infra = labels
        .get("nbot.infra")
        .map(parse_label_bool)
        .unwrap_or(false);
    if !is_infra {
        return None;
    }

    let name = labels
        .get("nbot.infra.name")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(service_id)
        .to_string();

    let description = labels
        .get("nbot.infra.description")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    Some(ToolContainer {
        id: service_id.to_string(),
        name,
        description,
        kind: "infra".to_string(),
        container_name: service_id.to_string(),
        status: "notfound".to_string(),
        ports: parse_published_ports(service),
        detail: None,
        container_id: None,
    })
}
