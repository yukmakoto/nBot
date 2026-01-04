use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

pub(super) struct TempFileGuard {
    pub(super) path: PathBuf,
}

impl TempFileGuard {
    pub(super) async fn new(prefix: &str, file_name: Option<&str>) -> Result<Self, String> {
        let mut dir = std::env::temp_dir();
        dir.push("nbot");
        dir.push("llm");
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| format!("Create temp dir failed: {e}"))?;

        let nonce: String = {
            use rand::distr::Alphanumeric;
            use rand::Rng;
            rand::rng()
                .sample_iter(Alphanumeric)
                .take(12)
                .map(char::from)
                .collect()
        };

        let safe_name = file_name
            .and_then(|s| Path::new(s).file_name().and_then(|n| n.to_str()))
            .map(sanitize_filename)
            .unwrap_or_else(|| "download.txt".to_string());

        let file_name = format!("{prefix}_{nonce}_{safe_name}");
        Ok(Self {
            path: dir.join(file_name),
        })
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn sanitize_filename(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        let ok = c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ' ');
        out.push(if ok { c } else { '_' });
    }
    let trimmed = out.trim().to_string();
    if trimmed.is_empty() {
        "file.txt".to_string()
    } else {
        trimmed
    }
}

fn truncate_to_chars(s: &mut String, max_chars: usize) -> bool {
    if max_chars == 0 {
        s.clear();
        return true;
    }

    let mut count: usize = 0;
    let mut end_idx: usize = s.len();
    for (idx, _) in s.char_indices() {
        if count == max_chars {
            end_idx = idx;
            break;
        }
        count += 1;
    }

    if count >= max_chars && end_idx < s.len() {
        s.truncate(end_idx);
        return true;
    }
    false
}

pub(super) struct DocumentMeta {
    pub(super) title: String,
    pub(super) file_ext: Option<String>,
    pub(super) size_bytes: Option<u64>,
    pub(super) truncated: bool,
}

pub(super) async fn download_document_text(
    url: &str,
    file_name: Option<&str>,
    timeout_ms: u64,
    max_bytes: u64,
    max_chars: u64,
) -> Result<(TempFileGuard, String, DocumentMeta), String> {
    let guard = TempFileGuard::new("download", file_name).await?;

    let timeout = std::time::Duration::from_millis(timeout_ms.clamp(1000, 120000));
    let max_bytes = max_bytes.clamp(1024, 50_000_000);
    let max_chars = max_chars.clamp(1000, 200_000) as usize;

    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| format!("Download failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("Download failed: HTTP {}", resp.status()));
    }

    let mut file = tokio::fs::File::create(&guard.path)
        .await
        .map_err(|e| format!("Create temp file failed: {e}"))?;

    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut truncated_by_bytes = false;
    let mut buf: Vec<u8> = Vec::new();

    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Read download stream failed: {e}"))?;
        if downloaded >= max_bytes {
            truncated_by_bytes = true;
            break;
        }

        let remaining = (max_bytes - downloaded) as usize;
        let slice: &[u8] = if chunk.len() > remaining {
            truncated_by_bytes = true;
            &chunk[..remaining]
        } else {
            &chunk
        };

        file.write_all(slice)
            .await
            .map_err(|e| format!("Write temp file failed: {e}"))?;
        buf.extend_from_slice(slice);
        downloaded += slice.len() as u64;

        if truncated_by_bytes {
            break;
        }
    }

    let mut text = String::from_utf8_lossy(&buf).to_string();
    let truncated_by_chars = truncate_to_chars(&mut text, max_chars);

    let meta = DocumentMeta {
        title: String::new(),
        file_ext: file_name
            .and_then(|s| Path::new(s).extension().and_then(|e| e.to_str()))
            .map(|s| s.to_lowercase()),
        size_bytes: Some(downloaded),
        truncated: truncated_by_bytes || truncated_by_chars,
    };

    Ok((guard, text, meta))
}
