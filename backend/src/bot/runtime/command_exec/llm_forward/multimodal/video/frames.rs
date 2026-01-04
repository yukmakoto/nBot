use std::path::{Path, PathBuf};
use tracing::warn;

use super::super::common::nonce12;
use super::super::image::{prepare_image_data_url, PreparedImageMeta};
use super::ffmpeg::run_program;

pub(super) fn evenly_spaced_indices(total: usize, keep: usize) -> Vec<usize> {
    if keep == 0 || total == 0 {
        return Vec::new();
    }
    if keep >= total {
        return (0..total).collect();
    }
    if keep == 1 {
        return vec![total / 2];
    }
    let denom = keep.saturating_sub(1);
    (0..keep)
        .map(|i| i.saturating_mul(total.saturating_sub(1)) / denom)
        .collect()
}

pub(super) async fn probe_video_duration_seconds(video_path: &Path) -> Option<f64> {
    let work_dir = video_path.parent()?;
    let file_name = video_path.file_name()?.to_str()?;

    let out = run_program(
        "ffprobe",
        work_dir,
        &[
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            file_name,
        ],
    )
    .await
    .ok()?;

    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    s.trim().parse::<f64>().ok()
}

pub(super) async fn extract_video_frames_as_data_urls(
    video_path: &Path,
    max_frames: u32,
    max_width: u32,
    max_height: u32,
    jpeg_quality: u8,
    frame_max_output_bytes: u64,
) -> Result<Vec<(u64, String, PreparedImageMeta)>, String> {
    let work_dir = video_path
        .parent()
        .ok_or_else(|| "视频路径无父目录".to_string())?;
    let input_name = video_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "视频文件名无效".to_string())?;

    let duration = probe_video_duration_seconds(video_path)
        .await
        .unwrap_or(0.0);
    let max_frames = max_frames.clamp(1, 24);

    let timestamps: Vec<f64> = if duration > 0.1 {
        (0..max_frames)
            .map(|i| ((i as f64 + 0.5) / max_frames as f64) * duration)
            .collect()
    } else {
        (0..max_frames).map(|i| i as f64).collect()
    };

    let tmp_dir_name = format!("frames_{}", nonce12());
    let tmp_dir = work_dir.join(&tmp_dir_name);
    tokio::fs::create_dir_all(&tmp_dir)
        .await
        .map_err(|e| format!("Create temp dir failed: {e}"))?;

    let mut frames: Vec<(u64, String, PreparedImageMeta)> = Vec::new();
    for (idx, ts) in timestamps.iter().enumerate() {
        let out_file = format!("frame_{idx}.png");
        let out_rel = format!("{tmp_dir_name}/{out_file}");
        let out_path = tmp_dir.join(&out_file);
        let scale = format!(
            "scale=w='min({max_width},iw)':h='min({max_height},ih)':force_original_aspect_ratio=decrease"
        );
        let ts_str = format!("{:.3}", ts);

        let res = run_program(
            "ffmpeg",
            work_dir,
            &[
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-ss",
                &ts_str,
                "-i",
                input_name,
                "-frames:v",
                "1",
                "-vf",
                &scale,
                "-vcodec",
                "png",
                &out_rel,
            ],
        )
        .await?;

        if !res.status.success() {
            let err = String::from_utf8_lossy(&res.stderr);
            warn!("ffmpeg extract frame failed: {}", err);
            continue;
        }

        let (data_url, meta) = prepare_image_data_url(
            &out_path,
            max_width,
            max_height,
            jpeg_quality,
            frame_max_output_bytes,
        )
        .await?;
        let _ = tokio::fs::remove_file(&out_path).await;

        let ts_ms = (*ts * 1000.0).max(0.0) as u64;
        frames.push((ts_ms, data_url, meta));
    }

    let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
    Ok(frames)
}

pub(super) async fn extract_audio_wav(
    video_path: &Path,
    max_audio_seconds: u32,
) -> Result<PathBufGuard, String> {
    let max_audio_seconds = max_audio_seconds.clamp(10, 1800);

    let work_dir = video_path
        .parent()
        .ok_or_else(|| "视频路径无父目录".to_string())?;
    let input_name = video_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "视频文件名无效".to_string())?;

    tokio::fs::create_dir_all(work_dir)
        .await
        .map_err(|e| format!("Create temp dir failed: {e}"))?;

    let out_file = format!("audio_{}.wav", nonce12());
    let out_path = work_dir.join(&out_file);

    let res = run_program(
        "ffmpeg",
        work_dir,
        &[
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-i",
            input_name,
            "-t",
            &max_audio_seconds.to_string(),
            "-vn",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-f",
            "wav",
            &out_file,
        ],
    )
    .await?;

    if !res.status.success() {
        let err = String::from_utf8_lossy(&res.stderr);
        return Err(format!("提取音频失败: {}", err));
    }

    Ok(PathBufGuard { path: out_path })
}

pub(super) struct PathBufGuard {
    pub(super) path: PathBuf,
}

impl Drop for PathBufGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
