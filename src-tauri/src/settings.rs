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
    /// When set, short dictations are cleaned by a fast local rule pass instead
    /// of the LLM formatter.
    ///
    /// The field-level `serde(default)` is load-bearing: [`load`] falls back to
    /// [`Settings::default`] on ANY deserialization error, so a settings file
    /// written by an older build (without this key) must default the field in
    /// place rather than resetting every other setting to its default.
    #[serde(default = "default_quick_clean_enabled")]
    pub quick_clean_enabled: bool,
    /// Upper word-count bound (exclusive) for quick-clean eligibility. Longer
    /// dictations always go through the LLM formatter.
    #[serde(default = "default_quick_clean_max_words")]
    pub quick_clean_max_words: u32,
}

fn default_quick_clean_enabled() -> bool {
    true
}

fn default_quick_clean_max_words() -> u32 {
    12
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
            quick_clean_enabled: default_quick_clean_enabled(),
            quick_clean_max_words: default_quick_clean_max_words(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v0_1_0_settings_deserialize_with_defaulted_quick_clean() {
        // A settings file written by v0.1.0 has no quick_clean_* keys and
        // carries non-default values for existing fields. Field-level
        // serde(default) must preserve every existing value AND default the new
        // fields — never fall back to Settings::default() for the whole struct.
        let raw = r#"{
            "stt_engine": "groq",
            "whisper_model": "base",
            "language": "en",
            "formatter": "groq",
            "ollama_model": "qwen3:8b",
            "groq_api_key": "sk-test",
            "groq_llm_model": "llama-3.3-70b-versatile",
            "hotkey": "Ctrl+Shift+D"
        }"#;

        let settings: Settings =
            serde_json::from_str(raw).expect("legacy settings must still parse");

        // Existing values are preserved (not reset by a whole-struct default).
        assert_eq!(settings.stt_engine, SttEngine::Groq);
        assert_eq!(settings.whisper_model, "base");
        assert_eq!(settings.language, "en");
        assert_eq!(settings.formatter, Formatter::Groq);
        assert_eq!(settings.ollama_model, "qwen3:8b");
        assert_eq!(settings.groq_api_key, "sk-test");
        assert_eq!(settings.groq_llm_model, "llama-3.3-70b-versatile");
        assert_eq!(settings.hotkey, "Ctrl+Shift+D");

        // New fields fall back to their field-level defaults.
        assert!(settings.quick_clean_enabled);
        assert_eq!(settings.quick_clean_max_words, 12);
    }
}
