use crate::command::CommandAction;
use crate::models::SharedState;
use crate::render_image::{load_png_base64_data_uri, render_html_to_image_base64};
use crate::utils::emoji_to_twemoji;

const TEMPLATE_PATH: &str = "assets/help_template.html";

fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn owner_label(state: &SharedState, cmd: &crate::command::Command) -> String {
    if cmd.is_builtin {
        return "内置".to_string();
    }

    match &cmd.action {
        CommandAction::Plugin(plugin_id) => state
            .plugins
            .get(plugin_id)
            .map(|p| p.manifest.name)
            .unwrap_or_else(|| plugin_id.to_string()),
        CommandAction::Custom(_) => "自定义".to_string(),
        CommandAction::Help => "内置".to_string(),
    }
}

pub async fn generate_help_image(state: &SharedState, bot_id: &str) -> Result<String, String> {
    let prefix = super::message::get_command_prefix(state, bot_id);
    let commands = state.commands.list();
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    // 读取模板文件
    let template = std::fs::read_to_string(TEMPLATE_PATH)
        .map_err(|e| format!("读取帮助模板失败: {} (路径: {})", e, TEMPLATE_PATH))?;

    // De-duplicate by command name (case-insensitive), prefer builtin when conflicts exist.
    let mut unique: std::collections::BTreeMap<String, crate::command::Command> =
        std::collections::BTreeMap::new();
    for cmd in commands.iter() {
        let key = cmd.name.trim().to_ascii_lowercase();
        let p = if cmd.is_builtin {
            3u8
        } else {
            match cmd.action {
                CommandAction::Plugin(_) => 2,
                CommandAction::Custom(_) => 1,
                CommandAction::Help => 3,
            }
        };

        match unique.get(&key) {
            None => {
                unique.insert(key, cmd.clone());
            }
            Some(existing) => {
                let ep = if existing.is_builtin {
                    3u8
                } else {
                    match existing.action {
                        CommandAction::Plugin(_) => 2,
                        CommandAction::Custom(_) => 1,
                        CommandAction::Help => 3,
                    }
                };
                if p > ep || (p == ep && cmd.id < existing.id) {
                    unique.insert(key, cmd.clone());
                }
            }
        }
    }

    let commands: Vec<crate::command::Command> = unique.into_values().collect();

    // Group commands by category
    let mut categories: std::collections::BTreeMap<String, Vec<crate::command::Command>> =
        std::collections::BTreeMap::new();
    for cmd in commands.iter() {
        categories
            .entry(cmd.category.clone())
            .or_default()
            .push(cmd.clone());
    }

    let enabled_plugins = state.plugins.list_enabled();
    let no_command_plugins: Vec<_> = enabled_plugins
        .into_iter()
        .filter(|p| p.manifest.commands.is_empty())
        .collect();

    let total_commands = commands.len();
    let total_features = total_commands + no_command_plugins.len();
    let available_commands = total_commands;

    // 生成分类 HTML
    let mut cat_html = String::new();
    for (name, cmds) in categories {
        let cat_name = escape_html(&name);
        let mut items_html = String::new();
        for (i, cmd) in cmds.iter().enumerate() {
            let alias_dot = if cmd.aliases.is_empty() {
                String::new()
            } else {
                r#"<div class="alias-tag"></div>"#.to_string()
            };
            let owner = escape_html(&owner_label(state, cmd));
            let owner_tag = format!(r#"<div class="owner-tag">{}</div>"#, owner);
            let cmd_name = escape_html(&cmd.name);
            let prefix_html = escape_html(&prefix);
            items_html.push_str(&format!(
                r#"
                <div class="cmd-card">
                    <div class="cmd-left">
                        <span class="cmd-num">{:02}</span>
                        <span class="cmd-name">{}{}</span>
                    </div>
                    <div class="cmd-right">
                        {}
                        {}
                    </div>
                </div>
            "#,
                i + 1,
                prefix_html,
                cmd_name,
                owner_tag,
                alias_dot,
            ));
        }

        cat_html.push_str(&format!(
            r#"
            <div class="category">
                <div class="cat-header">
                    <span class="cat-title">{}<span class="cat-count">{}</span></span>
                    <div class="cat-line"></div>
                </div>
                <div class="cmd-grid">
                    {}
                </div>
            </div>
        "#,
            cat_name,
            cmds.len(),
            items_html
        ));
    }

    let mut plugins_html = String::new();
    if !no_command_plugins.is_empty() {
        let mut items = String::new();
        for plugin in no_command_plugins.iter() {
            let title = escape_html(&plugin.manifest.name);
            let desc_raw = plugin.manifest.description.trim();
            let desc = if desc_raw.is_empty() {
                "（缺少简介：请在 manifest.json 填写 description）".to_string()
            } else {
                escape_html(desc_raw)
            };
            items.push_str(&format!(
                r#"
                <div class="plugin-card">
                    <div class="plugin-name">{}</div>
                    <div class="plugin-desc">{}</div>
                </div>
            "#,
                title, desc
            ));
        }

        plugins_html = format!(
            r#"
            <div class="plugin-section">
                <div class="cat-header">
                    <span class="cat-title">插件功能<span class="cat-count">{}</span></span>
                    <div class="cat-line"></div>
                </div>
                <div class="plugin-list">
                    {}
                </div>
            </div>
        "#,
            no_command_plugins.len(),
            items
        );
    }

    // 读取图片资源
    let logo_base64 = load_png_base64_data_uri("assets/nbot_logo.png")?;

    // 替换模板占位符
    let html = template
        .replace("{total_commands}", &total_features.to_string())
        .replace("{available_commands}", &available_commands.to_string())
        .replace("{categories_html}", &cat_html)
        .replace("{plugins_html}", &plugins_html)
        .replace("{current_time}", &now)
        .replace("{logo_base64}", &logo_base64);

    // 转换 emoji 为 Twemoji 图片
    let html = emoji_to_twemoji(&html);

    render_html_to_image_base64(html, 420, 90).await
}
