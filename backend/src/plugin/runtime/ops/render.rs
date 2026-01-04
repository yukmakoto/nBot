use deno_core::op2;

use crate::render_image::render_markdown_image;

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
