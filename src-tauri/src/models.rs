use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};

pub struct ModelInfo {
    pub key: &'static str,
    pub label: &'static str,
    pub file: &'static str,
    pub size_mb: u64,
}

/// Quantized ggml builds hosted by ggml.ai on Hugging Face.
pub const REGISTRY: &[ModelInfo] = &[
    ModelInfo {
        key: "large-v3-turbo-q5_0",
        label: "Large v3 Turbo (recommended — best quality/speed)",
        file: "ggml-large-v3-turbo-q5_0.bin",
        size_mb: 574,
    },
    ModelInfo {
        key: "small-q5_1",
        label: "Small (lighter, faster, less accurate)",
        file: "ggml-small-q5_1.bin",
        size_mb: 190,
    },
    ModelInfo {
        key: "base-q5_1",
        label: "Base (tiny footprint, quick tests)",
        file: "ggml-base-q5_1.bin",
        size_mb: 60,
    },
];

const HF_BASE: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

pub fn models_dir() -> PathBuf {
    crate::settings::config_dir().join("models")
}

pub fn find(key: &str) -> Option<&'static ModelInfo> {
    REGISTRY.iter().find(|m| m.key == key)
}

pub fn local_path(key: &str) -> Option<PathBuf> {
    find(key).map(|m| models_dir().join(m.file))
}

pub fn is_downloaded(key: &str) -> bool {
    local_path(key).map(|p| p.exists()).unwrap_or(false)
}

#[derive(Clone, Serialize)]
pub struct DownloadProgress {
    pub model: String,
    pub downloaded: u64,
    pub total: u64,
    pub done: bool,
}

/// Downloads a model into the config dir, emitting `flow://download-progress`
/// events so the settings UI can render a progress bar.
pub async fn download(app: AppHandle, key: String) -> Result<()> {
    let info = find(&key).ok_or_else(|| anyhow!("unknown model: {key}"))?;
    let dir = models_dir();
    tokio::fs::create_dir_all(&dir).await?;
    let dest = dir.join(info.file);
    if dest.exists() {
        return Ok(());
    }

    let url = format!("{HF_BASE}/{}", info.file);
    let response = reqwest::get(&url).await.context("download request failed")?;
    if !response.status().is_success() {
        return Err(anyhow!("download failed with status {}", response.status()));
    }
    let total = response.content_length().unwrap_or(info.size_mb * 1_048_576);

    let tmp = dest.with_extension("part");
    let mut file = tokio::fs::File::create(&tmp).await?;
    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut last_emit: u64 = 0;

    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("download interrupted")?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        // throttle events to every ~2MB
        if downloaded - last_emit > 2 * 1_048_576 {
            last_emit = downloaded;
            let _ = app.emit(
                "flow://download-progress",
                DownloadProgress {
                    model: key.clone(),
                    downloaded,
                    total,
                    done: false,
                },
            );
        }
    }
    file.flush().await?;
    drop(file);
    tokio::fs::rename(&tmp, &dest).await?;

    let _ = app.emit(
        "flow://download-progress",
        DownloadProgress {
            model: key,
            downloaded,
            total,
            done: true,
        },
    );
    Ok(())
}
