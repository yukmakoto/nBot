use crate::plugin::types::PluginManifest;
use flate2::read::GzDecoder;
use std::io::{Cursor, Read};
use std::path::{Component, PathBuf};
use tar::Archive;
use tracing::info;

/// 插件包 (.nbp) 解析器
/// 格式: tar.gz 包含 manifest.json 以及任意文件树（支持多文件/目录插件）。
pub struct PluginPackage {
    pub manifest: PluginManifest,
    /// Files in the package, excluding `manifest.json`.
    /// Paths are relative, use `/` separators.
    pub files: Vec<PluginPackageFile>,
}

#[derive(Debug, Clone)]
pub struct PluginPackageFile {
    pub path: String,
    pub data: Vec<u8>,
}

fn sanitize_tar_path(path: &PathBuf) -> Result<PathBuf, String> {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Normal(seg) => out.push(seg),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("Unsafe path in package: {}", path.to_string_lossy()));
            }
        }
    }
    Ok(out)
}

fn normalize_rel_path(path: &PathBuf) -> Result<String, String> {
    let sanitized = sanitize_tar_path(path)?;
    let s = sanitized.to_string_lossy().replace('\\', "/");
    let s = s.trim_start_matches("./").to_string();
    if s.is_empty() {
        return Err("Empty path in package".to_string());
    }
    Ok(s)
}

fn strip_common_root(files: &mut Vec<PluginPackageFile>, manifest_path: &str) {
    let mut first_components: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for f in files.iter() {
        let first = f.path.split('/').next().unwrap_or("").to_string();
        if !first.is_empty() {
            first_components.insert(first);
        }
    }

    // If everything is under a single top-level folder, strip it.
    if first_components.len() != 1 {
        return;
    }
    let root = match first_components.into_iter().next() {
        Some(r) => r,
        None => return,
    };

    // Don't strip if manifest is already at root.
    if manifest_path == "manifest.json" {
        return;
    }

    for f in files.iter_mut() {
        if let Some(stripped) = f.path.strip_prefix(&(root.clone() + "/")) {
            f.path = stripped.to_string();
        }
    }
}

impl PluginPackage {
    /// 从 .nbp 文件内容解析插件包
    pub fn from_bytes(data: &[u8]) -> Result<Self, String> {
        let cursor = Cursor::new(data);
        let gz = GzDecoder::new(cursor);
        let mut archive = Archive::new(gz);

        let mut manifest: Option<PluginManifest> = None;
        let mut manifest_rel_path: Option<String> = None;
        let mut files: Vec<PluginPackageFile> = Vec::new();

        for entry in archive
            .entries()
            .map_err(|e| format!("Failed to read archive: {}", e))?
        {
            let mut entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
            let path = entry
                .path()
                .map_err(|e| format!("Failed to get path: {}", e))?;
            let rel = normalize_rel_path(&path.to_path_buf())?;

            // Skip directories; only keep regular files.
            if entry.header().entry_type().is_dir() {
                continue;
            }

            if rel.ends_with("manifest.json") {
                // Only accept a single manifest.
                if manifest.is_some() {
                    return Err("Multiple manifest.json found in package".to_string());
                }
                let mut buf: Vec<u8> = Vec::new();
                entry
                    .read_to_end(&mut buf)
                    .map_err(|e| format!("Failed to read manifest: {}", e))?;
                let content = String::from_utf8(buf)
                    .map_err(|e| format!("Invalid manifest encoding: {}", e))?;
                manifest = Some(
                    serde_json::from_str(&content).map_err(|e| format!("Invalid manifest: {}", e))?,
                );
                manifest_rel_path = Some(rel);
                continue;
            }

            let mut buf: Vec<u8> = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| format!("Failed to read file: {}", e))?;
            files.push(PluginPackageFile { path: rel, data: buf });
        }

        let manifest = manifest.ok_or("Missing manifest.json")?;
        let manifest_rel_path = manifest_rel_path.unwrap_or_else(|| "manifest.json".to_string());

        strip_common_root(&mut files, &manifest_rel_path);

        // Ensure the declared entry exists (file or `<entry>/index.js`).
        let entry = manifest.entry.trim();
        let entry_candidates: Vec<String> = if entry.is_empty() {
            vec!["index.js".to_string()]
        } else {
            vec![entry.to_string(), format!("{}/index.js", entry.trim_end_matches('/'))]
        };
        let has_entry = files
            .iter()
            .any(|f| entry_candidates.iter().any(|c| c == &f.path));
        if !has_entry {
            return Err(format!(
                "Missing entry file in package: {}",
                entry_candidates.join(" or ")
            ));
        }

        info!("解析插件包: {} v{}", manifest.id, manifest.version);

        Ok(Self { manifest, files })
    }
}
