use crate::settings::{Formatter, Settings};
use anyhow::{anyhow, Context, Result};
use serde_json::json;
use std::time::Duration;

/// This prompt replicates Wispr Flow's post-processing pass: a small instruct
/// model rewrites the raw transcript into clean, intent-aware text. The
/// examples are sent as real user/assistant turns — small models (gemma3:4b)
/// slip into assistant mode and answer the transcript when the examples only
/// live inside the system prompt. Both files are shared with scripts/*.mjs.
const SYSTEM_PROMPT: &str = include_str!("../prompts/system_prompt.txt");
const FEW_SHOT_JSON: &str = include_str!("../prompts/few_shot.json");

fn build_messages(transcript: &str) -> Vec<serde_json::Value> {
    let mut messages = vec![json!({ "role": "system", "content": SYSTEM_PROMPT })];
    let shots: Vec<serde_json::Value> =
        serde_json::from_str(FEW_SHOT_JSON).expect("invalid few_shot.json");
    messages.extend(shots);
    messages.push(json!({ "role": "user", "content": transcript }));
    messages
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .expect("failed to build http client")
}

async fn format_ollama(model: &str, transcript: &str) -> Result<String> {
    let body = json!({
        "model": model,
        "stream": false,
        // keep the model resident so dictation after idle doesn't pay a reload
        "keep_alive": "60m",
        "options": { "temperature": 0.1 },
        "messages": build_messages(transcript)
    });
    let response = http()
        .post("http://localhost:11434/api/chat")
        .json(&body)
        .send()
        .await
        .context("Ollama is not reachable on localhost:11434 — is it running?")?;
    let status = response.status();
    let body: serde_json::Value = response.json().await.context("invalid Ollama response")?;
    if !status.is_success() {
        let msg = body["error"].as_str().unwrap_or("unknown error");
        return Err(anyhow!("Ollama error ({status}): {msg}"));
    }
    let text = body["message"]["content"].as_str().unwrap_or_default();
    Ok(strip_reasoning(text).trim().to_string())
}

async fn format_groq(api_key: &str, model: &str, transcript: &str) -> Result<String> {
    if api_key.is_empty() {
        return Err(anyhow!("Groq API key is not set"));
    }
    let body = json!({
        "model": model,
        "temperature": 0.1,
        "messages": build_messages(transcript)
    });
    let response = http()
        .post("https://api.groq.com/openai/v1/chat/completions")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .context("Groq request failed")?;
    let status = response.status();
    let body: serde_json::Value = response.json().await.context("invalid Groq response")?;
    if !status.is_success() {
        let msg = body["error"]["message"].as_str().unwrap_or("unknown error");
        return Err(anyhow!("Groq error ({status}): {msg}"));
    }
    let text = body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default();
    Ok(strip_reasoning(text).trim().to_string())
}

/// Some local models (qwen3 et al.) emit <think>...</think> blocks.
fn strip_reasoning(text: &str) -> String {
    match (text.find("<think>"), text.find("</think>")) {
        (Some(start), Some(end)) if end > start => {
            let mut out = String::new();
            out.push_str(&text[..start]);
            out.push_str(&text[end + "</think>".len()..]);
            out
        }
        _ => text.to_string(),
    }
}

/// Cleaned dictation keeps most of the speaker's words. When a small model
/// slips into assistant mode anyway ("Okay, I understand...") or invents
/// content, its output shares almost no vocabulary with the transcript —
/// those outputs are discarded in favor of the raw text.
fn keeps_speaker_words(transcript: &str, formatted: &str) -> bool {
    fn words(s: &str) -> std::collections::HashSet<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect()
    }
    let spoken = words(transcript);
    let output = words(formatted);
    if output.is_empty() {
        return false;
    }
    let kept = output.iter().filter(|w| spoken.contains(*w)).count();
    kept * 2 >= output.len() // at least half the output vocabulary was spoken
}

/// Formats the transcript, falling back to the raw text on any failure so a
/// dead Ollama or an exhausted rate limit never blocks dictation.
pub async fn format(settings: &Settings, transcript: &str) -> String {
    if transcript.is_empty() {
        return String::new();
    }
    let result = match settings.formatter {
        Formatter::None => return transcript.to_string(),
        Formatter::Ollama => format_ollama(&settings.ollama_model, transcript).await,
        Formatter::Groq => {
            format_groq(&settings.groq_api_key, &settings.groq_llm_model, transcript).await
        }
    };
    match result {
        Ok(text) if !text.is_empty() => {
            if keeps_speaker_words(transcript, &text) {
                text
            } else {
                log::warn!("formatter output discarded (not the speaker's words): {text:?}");
                transcript.to_string()
            }
        }
        Ok(_) => transcript.to_string(),
        Err(err) => {
            log::warn!("formatting failed, using raw transcript: {err:#}");
            transcript.to_string()
        }
    }
}
