use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SttEngine {
    /// whisper.cpp running on-device (Metal). Fully offline and free.
    Local,
    /// Groq cloud Whisper (free tier). Fastest, needs an API key.
    Groq,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Formatter {
    /// Local LLM through Ollama. Offline and free.
    Ollama,
    /// Groq chat completions (free tier). Near-instant.
    Groq,
    /// Skip the formatting pass, inject the raw transcript.
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub stt_engine: SttEngine,
    /// Key into the model registry (see models.rs).
    pub whisper_model: String,
    /// "auto" lets whisper detect the language; otherwise an ISO 639-1 code.
    pub language: String,
    pub formatter: Formatter,
    pub ollama_model: String,
    pub groq_api_key: String,
    pub groq_llm_model: String,
    /// Accelerator in tauri-plugin-global-shortcut syntax. Hold to talk.
    pub hotkey: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            stt_engine: SttEngine::Local,
            whisper_model: "large-v3-turbo-q5_0".into(),
            language: "auto".into(),
            formatter: Formatter::Ollama,
            ollama_model: "gemma3:4b".into(),
            groq_api_key: String::new(),
            groq_llm_model: "llama-3.1-8b-instant".into(),
            hotkey: "Alt+Space".into(),
        }
    }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("flow")
}

fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

pub fn load() -> Settings {
    let path = settings_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save(settings: &Settings) -> anyhow::Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let raw = serde_json::to_string_pretty(settings)?;
    std::fs::write(settings_path(), raw)?;
    Ok(())
}
