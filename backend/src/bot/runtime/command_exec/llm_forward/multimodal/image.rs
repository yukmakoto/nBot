use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, GenericImageView};
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

use crate::bot::runtime::BotRuntime;
use crate::models::SharedState;

use super::super::LlmForwardImageFromUrlInput;
use super::common::{
    call_chat_completions, download_binary_to_temp, log_llm_error, log_llm_len, reply_err,
    resolve_llm_config_by_name, send_llm_markdown_as_forward_image, SendForwardImageInput,
};

#[derive(Debug, Clone)]
pub(super) struct PreparedImageMeta {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) mime: String,
    pub(super) output_bytes: u64,
    pub(super) quality: u8,
}

fn composite_rgba_on_white(img: &DynamicImage) -> image::RgbImage {
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut rgb = image::RgbImage::new(w, h);

    for (x, y, px) in rgba.enumerate_pixels() {
        let [r, g, b, a] = px.0;
        let a_u16 = a as u16;
        let inv_a = 255u16 - a_u16;
        let out_r = ((r as u16 * a_u16) + (255u16 * inv_a)) / 255u16;
        let out_g = ((g as u16 * a_u16) + (255u16 * inv_a)) / 255u16;
        let out_b = ((b as u16 * a_u16) + (255u16 * inv_a)) / 255u16;
        rgb.put_pixel(x, y, image::Rgb([out_r as u8, out_g as u8, out_b as u8]));
    }

    rgb
}

fn encode_jpeg(rgb: &image::RgbImage, quality: u8) -> Result<Vec<u8>, String> {
    let mut out: Vec<u8> = Vec::new();
    let mut enc = JpegEncoder::new_with_quality(&mut out, quality);
    enc.encode(
        rgb.as_raw(),
        rgb.width(),
        rgb.height(),
        image::ExtendedColorType::Rgb8,
    )
    .map_err(|e| format!("JPEG encode failed: {e}"))?;
    Ok(out)
}

pub(super) async fn prepare_image_data_url(
    path: &Path,
    max_width: u32,
    max_height: u32,
    jpeg_quality: u8,
    max_output_bytes: u64,
) -> Result<(String, PreparedImageMeta), String> {
    let input = tokio::fs::read(path)
        .await
        .map_err(|e| format!("Read image failed: {e}"))?;

    let img = image::load_from_memory(&input).map_err(|e| format!("Decode image failed: {e}"))?;
    let (orig_w, orig_h) = img.dimensions();

    let (mut target_w, mut target_h) = (orig_w, orig_h);
    if orig_w > max_width || orig_h > max_height {
        let scale_w = max_width as f64 / orig_w as f64;
        let scale_h = max_height as f64 / orig_h as f64;
        let scale = scale_w.min(scale_h).min(1.0);
        target_w = (orig_w as f64 * scale).round().max(1.0) as u32;
        target_h = (orig_h as f64 * scale).round().max(1.0) as u32;
    }

    let mut rgb = if target_w != orig_w || target_h != orig_h {
        let resized = img.resize_exact(target_w, target_h, image::imageops::FilterType::Lanczos3);
        composite_rgba_on_white(&resized)
    } else {
        composite_rgba_on_white(&img)
    };

    let mut quality = jpeg_quality.clamp(30, 95);
    let max_output_bytes = max_output_bytes.clamp(50_000, 10_000_000) as usize;

    for _ in 0..10 {
        let jpeg = encode_jpeg(&rgb, quality)?;
        let jpeg_len = jpeg.len();
        if jpeg_len <= max_output_bytes {
            let b64 = BASE64.encode(&jpeg);
            return Ok((
                format!("data:image/jpeg;base64,{}", b64),
                PreparedImageMeta {
                    width: rgb.width(),
                    height: rgb.height(),
                    mime: "image/jpeg".to_string(),
                    output_bytes: jpeg_len as u64,
                    quality,
                },
            ));
        }

        if quality > 50 {
            quality = quality.saturating_sub(10).max(50);
            continue;
        }

        let new_w = ((rgb.width() as f64) * 0.85).round().max(1.0) as u32;
        let new_h = ((rgb.height() as f64) * 0.85).round().max(1.0) as u32;
        if new_w == rgb.width() && new_h == rgb.height() {
            break;
        }
        rgb = image::imageops::resize(&rgb, new_w, new_h, image::imageops::FilterType::Lanczos3);
        quality = jpeg_quality.clamp(30, 95);
    }

    Err(format!(
        "Image too large: cannot compress within {} bytes",
        max_output_bytes
    ))
}

pub(in super::super::super) async fn process_llm_forward_image_from_url(
    state: &SharedState,
    runtime: &Arc<BotRuntime>,
    bot_id: &str,
    input: LlmForwardImageFromUrlInput<'_>,
) {
    let user_id = input.user_id;
    let group_id = input.group_id;
    let system_prompt = input.system_prompt;
    let prompt = input.prompt;
    let title = input.title;

    let (guard, bin_meta) = match download_binary_to_temp(
        input.url,
        input.file_name,
        input.timeout_ms,
        input.max_bytes,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            reply_err(
                runtime,
                bot_id,
                user_id,
                group_id,
                &format!("下载失败：{e}"),
            )
            .await;
            return;
        }
    };

    let llm = match resolve_llm_config_by_name(state, bot_id, input.model_name) {
        Ok(v) => v,
        Err(e) => {
            reply_err(runtime, bot_id, user_id, group_id, &e).await;
            return;
        }
    };

    let (data_url, prepared_meta) = match prepare_image_data_url(
        &guard.path,
        input.max_width,
        input.max_height,
        input.jpeg_quality,
        input.max_output_bytes,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            reply_err(
                runtime,
                bot_id,
                user_id,
                group_id,
                &format!("图片处理失败：{e}"),
            )
            .await;
            return;
        }
    };

    let group_id_opt = (group_id != 0).then_some(group_id);
    let bot_name = state
        .bots
        .get(bot_id)
        .map(|b| b.value().name.clone())
        .unwrap_or_else(|| "nBot".to_string());
    let bot_platform = state
        .bots
        .get(bot_id)
        .map(|b| b.value().platform.clone())
        .unwrap_or_else(|| "unknown".to_string());
    let now = chrono::Local::now();

    let ctx = json!({
        "task": prompt,
        "title": title,
        "document": {
            "type": "image",
            "file_ext": bin_meta.file_ext,
            "size_bytes": bin_meta.size_bytes,
            "truncated": bin_meta.truncated,
            "prepared": {
                "mime": prepared_meta.mime,
                "width": prepared_meta.width,
                "height": prepared_meta.height,
                "bytes": prepared_meta.output_bytes,
                "jpeg_quality": prepared_meta.quality
            }
        },
        "environment": {
            "bot_id": bot_id,
            "bot_name": bot_name,
            "platform": bot_platform,
            "chat_type": if group_id_opt.is_some() { "group" } else { "private" },
            "time": now.to_rfc3339(),
        }
    });
    let ctx_pretty = serde_json::to_string_pretty(&ctx).unwrap_or_else(|_| ctx.to_string());

    let request_body = json!({
        "model": llm.model_name,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "system", "content": super::super::build_prompt_injection_guard()},
            {"role": "user", "content": [
                {"type": "text", "text": format!("上下文信息（JSON）：\n{}", ctx_pretty)},
                {"type": "image_url", "image_url": {"url": data_url}}
            ]}
        ],
        "max_tokens": 4096
    });

    let reply_content = match call_chat_completions(
        &llm.base_url,
        &llm.api_key,
        &request_body,
        llm.max_request_bytes,
    )
    .await
    {
        Ok(t) => t,
        Err(e) => {
            log_llm_error("图片分析", &e.to_string());
            reply_err(
                runtime,
                bot_id,
                user_id,
                group_id,
                &format!("分析失败：{e}"),
            )
            .await;
            return;
        }
    };

    log_llm_len("图片分析", reply_content.len());
    send_llm_markdown_as_forward_image(
        state,
        runtime,
        bot_id,
        SendForwardImageInput {
            user_id,
            group_id,
            title,
            markdown: &reply_content,
        },
    )
    .await;

    drop(guard);
}
