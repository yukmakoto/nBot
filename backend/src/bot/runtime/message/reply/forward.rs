use serde_json::{json, Value};
use std::collections::HashSet;

fn take_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.to_string()),
        Value::Number(n) => n.as_i64().map(|x| x.to_string()),
        _ => None,
    }
}

fn format_ts(ts: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(ts.max(0), 0)
        .single()
        .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| ts.to_string())
}

fn render_segment(seg: &Value, depth: usize, max_depth: usize) -> String {
    let seg_type = seg.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let data = seg.get("data").unwrap_or(&Value::Null);

    match seg_type {
        "text" => data
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        "at" => {
            let qq = data.get("qq").and_then(|v| v.as_str()).unwrap_or("");
            if qq == "all" {
                "@all".to_string()
            } else {
                "@用户".to_string()
            }
        }
        "image" => {
            let name = data
                .get("file")
                .or_else(|| data.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("image");
            let url = data.get("url").and_then(|v| v.as_str());
            match url {
                Some(u) => format!("[图片:{} {}]", name, u),
                None => format!("[图片:{}]", name),
            }
        }
        "video" => {
            let name = data
                .get("file")
                .or_else(|| data.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("video");
            let url = data.get("url").and_then(|v| v.as_str());
            match url {
                Some(u) => format!("[视频:{} {}]", name, u),
                None => format!("[视频:{}]", name),
            }
        }
        "record" => {
            let name = data
                .get("file")
                .or_else(|| data.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("record");
            let url = data.get("url").and_then(|v| v.as_str());
            match url {
                Some(u) => format!("[语音:{} {}]", name, u),
                None => format!("[语音:{}]", name),
            }
        }
        "file" => {
            let name = data
                .get("name")
                .or_else(|| data.get("file"))
                .and_then(|v| v.as_str())
                .unwrap_or("file");
            let url = data.get("url").and_then(|v| v.as_str());
            match url {
                Some(u) => format!("[文件:{} {}]", name, u),
                None => format!("[文件:{}]", name),
            }
        }
        "reply" => {
            let id = data.get("id").or_else(|| data.get("message_id"));
            let id_s = id.and_then(take_str).unwrap_or_else(|| "?".to_string());
            format!("[回复:{}]", id_s)
        }
        "markdown" => data
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        "forward" => {
            if depth >= max_depth {
                return "[转发消息]".to_string();
            }
            if let Some(content) = data.get("content").and_then(|v| v.as_array()) {
                let (txt, _) = render_forward_messages(
                    content,
                    super::FORWARD_TEXT_MAX_CHARS,
                    depth + 1,
                    max_depth,
                );
                format!("[转发消息]\n{}", txt)
            } else {
                let id = data.get("id").or_else(|| data.get("message_id"));
                let id_s = id.and_then(take_str).unwrap_or_else(|| "?".to_string());
                format!("[转发消息:{}]", id_s)
            }
        }
        _ => format!("[{}]", seg_type),
    }
}

fn render_message_text(msg: &Value, depth: usize, max_depth: usize) -> String {
    let message = msg.get("message").unwrap_or(&Value::Null);
    if let Some(s) = message.as_str() {
        return s.to_string();
    }
    let Some(arr) = message.as_array() else {
        return String::new();
    };

    let mut parts: Vec<String> = Vec::new();
    for seg in arr {
        let t = render_segment(seg, depth, max_depth);
        if !t.is_empty() {
            parts.push(t);
        }
    }
    parts.join("")
}

fn collect_forward_media_from_messages(
    messages: &[Value],
    depth: usize,
    max_depth: usize,
    out: &mut Vec<Value>,
    seen: &mut HashSet<String>,
    max_items: usize,
    truncated: &mut bool,
) {
    if depth >= max_depth || *truncated || out.len() >= max_items {
        *truncated = out.len() >= max_items;
        return;
    }

    for msg in messages {
        if *truncated || out.len() >= max_items {
            *truncated = out.len() >= max_items;
            return;
        }

        let message = msg.get("message").unwrap_or(&Value::Null);
        let Some(arr) = message.as_array() else {
            continue;
        };

        for seg in arr {
            if *truncated || out.len() >= max_items {
                *truncated = out.len() >= max_items;
                return;
            }

            let seg_type = seg.get("type").and_then(|t| t.as_str()).unwrap_or("");
            let data = seg.get("data").unwrap_or(&Value::Null);

            if seg_type == "forward" {
                if let Some(content) = data.get("content").and_then(|v| v.as_array()) {
                    collect_forward_media_from_messages(
                        content,
                        depth + 1,
                        max_depth,
                        out,
                        seen,
                        max_items,
                        truncated,
                    );
                }
                continue;
            }

            if !matches!(seg_type, "image" | "video" | "record" | "file") {
                continue;
            }

            let file = data.get("file").and_then(|v| v.as_str()).unwrap_or("");
            let name = data
                .get("name")
                .or_else(|| data.get("file"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let url = data
                .get("url")
                .and_then(|v| v.as_str())
                .map(super::super::decode_basic_html_entities)
                .unwrap_or_default();

            let key = if !url.is_empty() {
                format!("{seg_type}:{url}")
            } else if !file.is_empty() {
                format!("{seg_type}:file:{file}")
            } else {
                continue;
            };
            if seen.contains(&key) {
                continue;
            }
            seen.insert(key);

            let name_opt = if name.is_empty() { None } else { Some(name) };
            let file_opt = if file.is_empty() { None } else { Some(file) };
            let url_opt = if url.is_empty() { None } else { Some(url) };

            out.push(json!({
                "type": seg_type,
                "name": name_opt,
                "file": file_opt,
                "url": url_opt,
            }));
        }
    }
}

pub(super) fn collect_forward_media(messages: &[Value]) -> (Vec<Value>, bool) {
    let mut out: Vec<Value> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut truncated = false;
    collect_forward_media_from_messages(
        messages,
        0,
        super::FORWARD_MAX_DEPTH,
        &mut out,
        &mut seen,
        super::FORWARD_MEDIA_MAX_ITEMS,
        &mut truncated,
    );
    (out, truncated)
}

fn truncate_to_chars(s: &mut String, max_chars: usize) -> bool {
    if max_chars == 0 {
        s.clear();
        return true;
    }

    let mut count: usize = 0;
    let mut end_idx: usize = s.len();
    for (idx, _) in s.char_indices() {
        if count == max_chars {
            end_idx = idx;
            break;
        }
        count += 1;
    }

    if count >= max_chars && end_idx < s.len() {
        s.truncate(end_idx);
        return true;
    }
    false
}

pub(super) fn render_forward_messages(
    messages: &[Value],
    max_chars: usize,
    depth: usize,
    max_depth: usize,
) -> (String, bool) {
    let mut out = String::new();
    let mut truncated = false;
    let byte_soft_limit = max_chars.saturating_mul(4);

    for (idx, msg) in messages.iter().enumerate() {
        let nickname = msg
            .get("sender")
            .and_then(|s| s.get("nickname"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let time = msg.get("time").and_then(|v| v.as_i64()).unwrap_or(0);
        let header = format!("#{} {} {}\n", idx + 1, nickname, format_ts(time));
        out.push_str(&header);

        let body = render_message_text(msg, depth, max_depth);
        if !body.is_empty() {
            out.push_str(&body);
            if !body.ends_with('\n') {
                out.push('\n');
            }
        }
        out.push('\n');

        if out.len() >= byte_soft_limit {
            truncated = true;
            break;
        }
    }

    if truncate_to_chars(&mut out, max_chars) {
        truncated = true;
    }
    (out, truncated)
}

pub(super) fn extract_forward_id(message_arr: &Value, raw_message: &str) -> Option<String> {
    if let Some(segments) = message_arr.as_array() {
        if let Some(seg) = segments
            .iter()
            .find(|s| s.get("type").and_then(|t| t.as_str()) == Some("forward"))
        {
            if let Some(data) = seg.get("data") {
                let id = data.get("id").or_else(|| data.get("message_id"));
                if let Some(v) = id.and_then(take_str) {
                    return Some(v);
                }
            }
        }
    }

    if raw_message.contains("[CQ:forward") {
        return super::super::parse_cq_field(raw_message, "id");
    }
    None
}
