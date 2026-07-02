use crate::settings::{Formatter, Settings};
use anyhow::{anyhow, Context, Result};
use serde_json::json;

/// This prompt replicates Wispr Flow's post-processing pass: a small instruct
/// model rewrites the raw transcript into clean, intent-aware text. The
/// examples are sent as real user/assistant turns — small models (gemma3:4b)
/// slip into assistant mode and answer the transcript when the examples only
/// live inside the system prompt. Both files are shared with scripts/*.mjs.
const SYSTEM_PROMPT: &str = include_str!("../prompts/system_prompt.txt");
const FEW_SHOT_JSON: &str = include_str!("../prompts/few_shot.json");

fn build_messages(
    transcript: &str,
    terms: &[String],
    style: Option<(crate::prompt::Tone, crate::prompt::Context)>,
) -> Vec<serde_json::Value> {
    // Augment the base system prompt with the user's proper nouns and the active
    // style fragment. With no terms and no style this is the identity of
    // SYSTEM_PROMPT, so behavior is unchanged from before management-ui.
    let fragment = style.map(|(tone, context)| crate::prompt::style_fragment(tone, context));
    let system = crate::prompt::augment_system_prompt(SYSTEM_PROMPT, terms, fragment);
    let mut messages = vec![json!({ "role": "system", "content": system })];
    let mut shots: Vec<serde_json::Value> =
        serde_json::from_str(FEW_SHOT_JSON).expect("invalid few_shot.json");
    // The register must be demonstrated, not described: every assistant
    // example is re-registered to the active tone so all shots agree — a
    // couple of style turns can't outweigh five capitalized base answers on
    // a small model.
    if let Some((tone, _)) = style {
        for shot in &mut shots {
            if shot["role"] == "assistant" {
                let content = shot["content"].as_str().unwrap_or_default();
                shot["content"] = json!(crate::prompt::apply_register(content, tone));
            }
        }
    }
    messages.extend(shots);
    if let Some((tone, _)) = style {
        for (input, output) in crate::prompt::style_shots(tone) {
            messages.push(json!({ "role": "user", "content": input }));
            messages.push(json!({ "role": "assistant", "content": output }));
        }
    }
    messages.push(json!({ "role": "user", "content": transcript }));
    messages
}

async fn format_ollama(
    model: &str,
    transcript: &str,
    terms: &[String],
    style: Option<(crate::prompt::Tone, crate::prompt::Context)>,
) -> Result<String> {
    let body = json!({
        "model": model,
        "stream": false,
        // keep the model resident so dictation after idle doesn't pay a reload
        "keep_alive": "60m",
        "options": { "temperature": 0.1 },
        "messages": build_messages(transcript, terms, style)
    });
    let response = crate::http::client()
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

async fn format_groq(
    api_key: &str,
    model: &str,
    transcript: &str,
    terms: &[String],
    style: Option<(crate::prompt::Tone, crate::prompt::Context)>,
) -> Result<String> {
    if api_key.is_empty() {
        return Err(anyhow!("Groq API key is not set"));
    }
    let body = json!({
        "model": model,
        "temperature": 0.1,
        "messages": build_messages(transcript, terms, style)
    });
    let response = crate::http::client()
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
pub async fn format(
    settings: &Settings,
    transcript: &str,
    terms: &[String],
    style: Option<(crate::prompt::Tone, crate::prompt::Context)>,
) -> String {
    if transcript.is_empty() {
        return String::new();
    }
    let result = match settings.formatter {
        Formatter::None => return transcript.to_string(),
        Formatter::Ollama => {
            format_ollama(&settings.ollama_model, transcript, terms, style).await
        }
        Formatter::Groq => {
            format_groq(
                &settings.groq_api_key,
                &settings.groq_llm_model,
                transcript,
                terms,
                style,
            )
            .await
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
