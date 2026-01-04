use crate::utils::emoji_to_twemoji;
use base64::Engine;
use comrak::{plugins::syntect::SyntectAdapter, ComrakOptions, ComrakPlugins};
use tracing::{info, warn};

const TEMPLATE_PATH: &str = "assets/report_template.html";

pub(crate) fn load_png_base64_data_uri(path: &str) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("读取图片失败: {} (路径: {})", e, path))?;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&data)
    ))
}

fn escape_html_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

pub(crate) async fn render_html_to_image_base64(
    html: String,
    width: u32,
    quality: u8,
) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct RenderResp {
        status: String,
        #[serde(default)]
        image: Option<String>,
        #[serde(default)]
        message: Option<String>,
    }

    let url =
        std::env::var("WKHTMLTOIMAGE_URL").unwrap_or_else(|_| "http://localhost:32180".to_string());
    info!("调用 wkhtmltoimage 服务: {}", url);

    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "html": html,
        "width": width,
        "quality": quality.clamp(10, 100),
        "format": "png"
    });

    let resp = client
        .post(format!("{}/render", url.trim_end_matches('/')))
        .json(&body)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| format!("调用 wkhtmltoimage 服务失败: {}", e))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取 wkhtmltoimage 响应失败: {}", e))?;

    if !status.is_success() {
        let snippet: String = text.chars().take(400).collect();
        return Err(format!(
            "wkhtmltoimage 服务返回错误 (HTTP {}): {}",
            status.as_u16(),
            snippet
        ));
    }

    let data: RenderResp = serde_json::from_str(&text).map_err(|e| {
        format!(
            "解析 wkhtmltoimage 响应失败: {}: {}",
            e,
            text.chars().take(200).collect::<String>()
        )
    })?;

    if data.status == "success" {
        data.image
            .ok_or_else(|| "wkhtmltoimage 响应缺少 image 字段".to_string())
    } else {
        let msg = data.message.unwrap_or_else(|| "未知错误".to_string());
        warn!("wkhtmltoimage 服务返回错误: {}", msg);
        Err(format!("wkhtmltoimage 渲染失败: {}", msg))
    }
}

fn markdown_to_html(markdown: &str) -> String {
    let mut options = ComrakOptions::default();
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.parse.smart = true;
    options.render.unsafe_ = false;

    let adapter = SyntectAdapter::new(Some("base16-ocean.dark"));
    let mut plugins = ComrakPlugins::default();
    plugins.render.codefence_syntax_highlighter = Some(&adapter);

    comrak::markdown_to_html_with_plugins(markdown, &options, &plugins)
}

pub async fn render_markdown_image(
    title: &str,
    meta: &str,
    markdown: &str,
    width: u32,
) -> Result<String, String> {
    let template = std::fs::read_to_string(TEMPLATE_PATH)
        .map_err(|e| format!("读取报告模板失败: {} (路径: {})", e, TEMPLATE_PATH))?;

    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let html_body = markdown_to_html(markdown);
    let logo_base64 = load_png_base64_data_uri("assets/nbot_logo.png")?;

    let html = template
        .replace("{title}", &escape_html_text(title))
        .replace("{meta}", &escape_html_text(meta))
        .replace("{time}", &now)
        .replace("{logo_base64}", &logo_base64)
        .replace("{content}", &html_body);

    let html = emoji_to_twemoji(&html);
    render_html_to_image_base64(html, width, 92).await
}
