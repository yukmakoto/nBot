use super::ToolContainer;
use std::io::ErrorKind;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

const DEFAULT_FFMPEG_IMAGE: &str = "jrottenberg/ffmpeg:6.1-alpine";

fn configured_ffmpeg_image() -> String {
    std::env::var("NBOT_FFMPEG_IMAGE")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_FFMPEG_IMAGE.to_string())
}

async fn try_get_local_ffmpeg_version_line() -> Result<Option<String>, String> {
    let program = super::ffmpeg_program();
    let out = timeout(
        Duration::from_secs(3),
        Command::new(&program).args(["-version"]).output(),
    )
    .await
    .map_err(|_| format!("{program} -version 超时"))?;

    match out {
        Ok(out) => {
            if !out.status.success() {
                return Err(format!(
                    "{program} -version 失败 (exit={}): {}",
                    out.status.code().unwrap_or(-1),
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
            }

            let first_line = String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .unwrap_or_default()
                .trim()
                .to_string();
            Ok(Some(if first_line.is_empty() {
                format!("{program} (version unknown)")
            } else {
                first_line
            }))
        }
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("运行 {program} 失败: {e}")),
    }
}

async fn docker_image_exists(image: &str) -> Result<bool, String> {
    let out = timeout(
        Duration::from_secs(6),
        Command::new("docker")
            .args(["image", "ls", "-q", image])
            .output(),
    )
    .await
    .map_err(|_| "docker image ls 超时".to_string())?;

    match out {
        Ok(out) if out.status.success() => {
            Ok(!String::from_utf8_lossy(&out.stdout).trim().is_empty())
        }
        Ok(out) => Err(format!(
            "docker image ls 失败 (exit={}): {}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        Err(e) => Err(format!("无法执行 docker: {e}")),
    }
}

pub async fn runner_tools() -> Vec<ToolContainer> {
    let image = configured_ffmpeg_image();

    let local = try_get_local_ffmpeg_version_line().await;
    let (status, local_ok, local_detail) = match local {
        Ok(Some(v)) => ("ready".to_string(), true, Some(format!("local={v}"))),
        Ok(None) => (
            "missing".to_string(),
            false,
            Some("local=missing".to_string()),
        ),
        Err(e) => (
            "error".to_string(),
            false,
            Some(format!("local error: {e}")),
        ),
    };

    // Docker image check is best-effort and never affects readiness.
    let docker_detail = match docker_image_exists(&image).await {
        Ok(true) => Some("docker image: present".to_string()),
        Ok(false) if local_ok => None,
        Ok(false) => Some("docker image: missing".to_string()),
        Err(_) if local_ok => None,
        Err(_) => Some("docker: unavailable".to_string()),
    };

    let mut detail_parts = Vec::<String>::new();
    detail_parts.push(format!("image={image}"));
    if let Some(v) = local_detail {
        detail_parts.push(v);
    }
    if let Some(d) = docker_detail {
        detail_parts.push(d);
    }

    vec![ToolContainer {
        id: "ffmpeg".to_string(),
        name: "ffmpeg".to_string(),
        description: "视频/音频处理（转码/压缩/抽帧）".to_string(),
        kind: "runner".to_string(),
        container_name: String::new(),
        status,
        ports: Vec::new(),
        detail: Some(detail_parts.join(" · ")),
        container_id: None,
    }]
}

pub fn ffmpeg_image_for_pull() -> String {
    configured_ffmpeg_image()
}

pub fn is_runner_tool_id(id: &str) -> bool {
    matches!(id, "ffmpeg")
}
