use std::path::Path;
use tracing::warn;

use super::super::super::download::TempFileGuard;
use super::super::common::read_file_as_data_url;
use super::ffmpeg::run_program;

#[derive(Debug, Clone)]
pub(super) struct PreparedVideoMeta {
    pub(super) original_bytes: u64,
    pub(super) prepared_bytes: u64,
    pub(super) transcoded: bool,
}

#[derive(Debug, Clone)]
pub(super) struct VideoTranscodeProfile {
    pub(super) max_width: u32,
    pub(super) max_height: u32,
    pub(super) crf: u8,
    pub(super) audio_kbps: Option<u32>,
}

pub(super) async fn prepare_video_data_url_with_budget(
    video_path: &Path,
    mime_name: &str,
    max_raw_bytes: u64,
) -> Result<(String, PreparedVideoMeta), String> {
    let original_bytes = tokio::fs::metadata(video_path)
        .await
        .map(|m| m.len())
        .map_err(|e| format!("读取视频大小失败: {e}"))?;

    // Always transcode video for consistent quality and smaller size
    // Only skip transcoding for very small videos (< 100KB) that are already within budget
    let skip_transcode_threshold = 100_000u64; // 100KB
    if original_bytes <= skip_transcode_threshold && original_bytes <= max_raw_bytes {
        let data_url = read_file_as_data_url(video_path, mime_name).await?;
        return Ok((
            data_url,
            PreparedVideoMeta {
                original_bytes,
                prepared_bytes: original_bytes,
                transcoded: false,
            },
        ));
    }

    let work_dir = video_path
        .parent()
        .ok_or_else(|| "视频路径无父目录".to_string())?;
    let input_name = video_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "视频文件名无效".to_string())?;

    let profiles: Vec<VideoTranscodeProfile> = vec![
        VideoTranscodeProfile {
            max_width: 960,
            max_height: 960,
            crf: 32,
            audio_kbps: Some(64),
        },
        VideoTranscodeProfile {
            max_width: 720,
            max_height: 720,
            crf: 34,
            audio_kbps: Some(48),
        },
        VideoTranscodeProfile {
            max_width: 640,
            max_height: 640,
            crf: 36,
            audio_kbps: Some(40),
        },
        VideoTranscodeProfile {
            max_width: 480,
            max_height: 480,
            crf: 38,
            audio_kbps: Some(32),
        },
        VideoTranscodeProfile {
            max_width: 360,
            max_height: 360,
            crf: 40,
            audio_kbps: None,
        },
    ];

    for profile in profiles.iter() {
        let out_guard = TempFileGuard::new("video", Some("video.mp4")).await?;
        let out_file = out_guard
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| "输出文件名无效".to_string())?
            .to_string();

        let vf = format!(
            "scale=w='min({},iw)':h='min({},ih)':force_original_aspect_ratio=decrease:force_divisible_by=2,format=yuv420p",
            profile.max_width, profile.max_height
        );
        let crf = profile.crf.to_string();

        let mut args: Vec<&str> = vec![
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-i",
            input_name,
            "-vf",
            &vf,
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            &crf,
            "-movflags",
            "+faststart",
        ];

        let audio_bitrate: Option<String> =
            profile.audio_kbps.map(|k| format!("{}k", k.clamp(16, 256)));
        if let Some(br) = audio_bitrate.as_deref() {
            args.extend_from_slice(&["-c:a", "aac", "-b:a", br]);
        } else {
            args.push("-an");
        }
        args.push(&out_file);

        let out = run_program("ffmpeg", work_dir, &args).await?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            warn!("ffmpeg transcode failed (profile {:?}): {}", profile, err);
            drop(out_guard);
            continue;
        }

        let prepared_bytes = tokio::fs::metadata(&out_guard.path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        if prepared_bytes == 0 {
            drop(out_guard);
            continue;
        }

        if prepared_bytes <= max_raw_bytes {
            let data_url = read_file_as_data_url(&out_guard.path, "video.mp4").await?;
            drop(out_guard);
            return Ok((
                data_url,
                PreparedVideoMeta {
                    original_bytes,
                    prepared_bytes,
                    transcoded: true,
                },
            ));
        }

        drop(out_guard);
    }

    Err(format!(
        "视频过大：{} bytes，无法压缩到 {} bytes 以内（请缩短视频、提高提供商的 max_request_bytes，或改用 frames 模式）",
        original_bytes, max_raw_bytes
    ))
}
