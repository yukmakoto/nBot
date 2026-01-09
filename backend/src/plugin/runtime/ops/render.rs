use deno_core::op2;

use crate::render_image::{render_html_to_image_base64, render_markdown_image};

// Op: Render Markdown to image (base64, async)
#[op2(async)]
#[string]
pub(in super::super) async fn op_render_markdown_image(
    #[string] title: String,
    #[string] meta: String,
    #[string] markdown: String,
    #[bigint] width: i64,
) -> Result<String, deno_core::error::AnyError> {
    let width_u32: u32 = width.clamp(320, 1200) as u32;
    render_markdown_image(&title, &meta, &markdown, width_u32)
        .await
        .map_err(deno_core::error::generic_error)
}

// Op: Render raw HTML to image (base64, async)
#[op2(async)]
#[string]
pub(in super::super) async fn op_render_html_image(
    #[string] html: String,
    #[bigint] width: i64,
    #[bigint] quality: i64,
) -> Result<String, deno_core::error::AnyError> {
    let width_u32: u32 = width.clamp(320, 2000) as u32;
    let quality_u8: u8 = quality.clamp(10, 100) as u8;
    render_html_to_image_base64(html, width_u32, quality_u8)
        .await
        .map_err(deno_core::error::generic_error)
}
