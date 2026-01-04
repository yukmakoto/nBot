fn normalize_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn sibling_program_path(program_path: &str, sibling_file: &str) -> Option<String> {
    let p = std::path::PathBuf::from(program_path);
    let dir = p.parent()?;
    Some(dir.join(sibling_file).to_string_lossy().to_string())
}

pub(crate) fn ffmpeg_program() -> String {
    normalize_env("NBOT_FFMPEG_BIN")
        .or_else(|| normalize_env("FFMPEG_BIN"))
        .unwrap_or_else(|| "ffmpeg".to_string())
}

pub(crate) fn ffprobe_program() -> String {
    normalize_env("NBOT_FFPROBE_BIN")
        .or_else(|| normalize_env("FFPROBE_BIN"))
        .or_else(|| {
            normalize_env("NBOT_FFMPEG_BIN").and_then(|ffmpeg| {
                let sibling = if cfg!(windows) {
                    "ffprobe.exe"
                } else {
                    "ffprobe"
                };
                sibling_program_path(&ffmpeg, sibling)
            })
        })
        .unwrap_or_else(|| "ffprobe".to_string())
}
