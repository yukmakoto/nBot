use serde_json::json;

use crate::models::SharedState;

use super::super::common::BinaryMeta;
use super::super::image::PreparedImageMeta;

pub(super) struct VideoCtxInput<'a> {
    pub(super) state: &'a SharedState,
    pub(super) bot_id: &'a str,
    pub(super) group_id: u64,
    pub(super) prompt: &'a str,
    pub(super) title: &'a str,
    pub(super) bin_meta: &'a BinaryMeta,
    pub(super) duration_seconds: Option<f64>,
    pub(super) frames: &'a [(u64, PreparedImageMeta)],
    pub(super) frames_total: usize,
    pub(super) frames_selected: usize,
    pub(super) transcript_included: bool,
}

pub(super) fn build_video_ctx(input: VideoCtxInput<'_>) -> serde_json::Value {
    let group_id_opt = (input.group_id != 0).then_some(input.group_id);
    let bot_name = input
        .state
        .bots
        .get(input.bot_id)
        .map(|b| b.value().name.clone())
        .unwrap_or_else(|| "nBot".to_string());
    let bot_platform = input
        .state
        .bots
        .get(input.bot_id)
        .map(|b| b.value().platform.clone())
        .unwrap_or_else(|| "unknown".to_string());
    let now = chrono::Local::now();

    json!({
        "task": input.prompt,
        "title": input.title,
        "document": {
            "type": "video",
            "file_ext": input.bin_meta.file_ext,
            "size_bytes": input.bin_meta.size_bytes,
            "truncated": input.bin_meta.truncated,
            "duration_seconds": input.duration_seconds,
            "frames_total": input.frames_total,
            "frames_selected": input.frames_selected,
            "frames": input.frames.iter().map(|(ts_ms, meta)| {
                json!({
                    "timestamp_ms": ts_ms,
                    "prepared": {
                        "mime": meta.mime,
                        "width": meta.width,
                        "height": meta.height,
                        "bytes": meta.output_bytes,
                        "jpeg_quality": meta.quality
                    }
                })
            }).collect::<Vec<_>>(),
            "transcript_included": input.transcript_included
        },
        "environment": {
            "bot_id": input.bot_id,
            "bot_name": bot_name,
            "platform": bot_platform,
            "chat_type": if group_id_opt.is_some() { "group" } else { "private" },
            "time": now.to_rfc3339(),
        }
    })
}
