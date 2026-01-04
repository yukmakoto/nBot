use std::collections::BTreeSet;

fn truncate_prefix(s: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out = s.chars().take(max_chars).collect::<String>();
    out.push_str("\n...(truncated)...");
    out
}

fn normalize_url_token(token: &str) -> Option<String> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }

    let start = token.find("https://").or_else(|| token.find("http://"))?;
    let mut s = &token[start..];

    // Trim common wrappers: markdown links, brackets, quotes.
    s = s.trim_start_matches(&['(', '[', '<', '"', '\''][..]);
    s = s.trim_end_matches(|c: char| {
        matches!(
            c,
            ',' | '.' | ';' | ':' | '!' | '?' | ')' | ']' | '>' | '"' | '\''
        )
    });

    if s.starts_with("http://") || s.starts_with("https://") {
        Some(s.to_string())
    } else {
        None
    }
}

pub(super) fn extract_urls(markdown: &str, max_urls: usize) -> Vec<String> {
    let mut set: BTreeSet<String> = BTreeSet::new();
    for token in markdown.split_whitespace() {
        if let Some(url) = normalize_url_token(token) {
            set.insert(url);
            if set.len() >= max_urls {
                break;
            }
        }
    }
    set.into_iter().collect()
}

pub(super) fn extract_fenced_code_blocks(markdown: &str, max_blocks: usize) -> Vec<String> {
    let mut blocks: Vec<String> = Vec::new();
    let mut in_block = false;
    let mut current = String::new();

    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            if in_block {
                let code = current.trim_end().to_string();
                if !code.is_empty() {
                    blocks.push(code);
                    if blocks.len() >= max_blocks {
                        break;
                    }
                }
                current.clear();
                in_block = false;
            } else {
                in_block = true;
                current.clear();
            }
            continue;
        }

        if in_block {
            current.push_str(line);
            current.push('\n');
        }
    }

    blocks
}

pub(super) fn build_plain_supplement_nodes(markdown: &str) -> Vec<String> {
    const MAX_URLS: usize = 10;
    const MAX_CODE_BLOCKS: usize = 3;
    const MAX_NODE_CHARS: usize = 2800;

    let urls = extract_urls(markdown, MAX_URLS);
    let codes = extract_fenced_code_blocks(markdown, MAX_CODE_BLOCKS);

    let mut nodes: Vec<String> = Vec::new();

    if !urls.is_empty() {
        let body = urls.join("\n");
        nodes.push(truncate_prefix(&format!("链接：\n{body}"), MAX_NODE_CHARS));
    }

    for (idx, code) in codes.into_iter().enumerate() {
        let label = format!("代码块 #{}：\n{}", idx + 1, code);
        nodes.push(truncate_prefix(&label, MAX_NODE_CHARS));
    }

    nodes
}
