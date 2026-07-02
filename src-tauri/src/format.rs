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

/// Prompt-engineer mode uses its own system prompt and few-shot pairs: the
/// model restructures a dictation into a well-formed AI prompt instead of
/// cleaning prose. Both files ship alongside the base pair.
const PE_SYSTEM_PROMPT: &str = include_str!("../prompts/prompt_engineer_system.txt");
const PE_FEW_SHOT_JSON: &str = include_str!("../prompts/prompt_engineer_few_shot.json");

fn build_messages(
    transcript: &str,
    terms: &[String],
    mode: &crate::prompt::Mode,
) -> Vec<serde_json::Value> {
    let messages = match mode {
        crate::prompt::Mode::Style(style) => build_style_messages(terms, *style),
        crate::prompt::Mode::PromptEngineer => build_prompt_engineer_messages(terms),
    };
    let mut messages = messages;
    messages.push(json!({ "role": "user", "content": transcript }));
    messages
}

/// The default dictation pass. Behavior is byte-identical to the pre-parity
/// `Option<(Tone, Context)>` path: with no terms and no style this is the
/// identity of SYSTEM_PROMPT + the base few-shot pairs.
fn build_style_messages(
    terms: &[String],
    style: Option<(crate::prompt::Tone, crate::prompt::Context)>,
) -> Vec<serde_json::Value> {
    // Augment the base system prompt with the user's proper nouns and the active
    // style fragment.
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
    messages
}

/// The prompt-engineer pass. The proper-noun preservation line is reused (no
/// style fragment), and the PE few-shot turns are sent verbatim — no register
/// rewriting, no style example turns.
fn build_prompt_engineer_messages(terms: &[String]) -> Vec<serde_json::Value> {
    let system = crate::prompt::augment_system_prompt(PE_SYSTEM_PROMPT, terms, None);
    let mut messages = vec![json!({ "role": "system", "content": system })];
    let shots: Vec<serde_json::Value> =
        serde_json::from_str(PE_FEW_SHOT_JSON).expect("invalid prompt_engineer_few_shot.json");
    messages.extend(shots);
    messages
}

async fn format_ollama(
    model: &str,
    transcript: &str,
    terms: &[String],
    mode: &crate::prompt::Mode,
) -> Result<String> {
    let body = json!({
        "model": model,
        "stream": false,
        // keep the model resident so dictation after idle doesn't pay a reload
        "keep_alive": "60m",
        "options": { "temperature": 0.1 },
        "messages": build_messages(transcript, terms, mode)
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
    mode: &crate::prompt::Mode,
) -> Result<String> {
    if api_key.is_empty() {
        return Err(anyhow!("Groq API key is not set"));
    }
    let body = json!({
        "model": model,
        "temperature": 0.1,
        "messages": build_messages(transcript, terms, mode)
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

/// Decides whether a non-empty formatter output is trusted or discarded in
/// favor of the raw transcript. Prompt-engineer mode restructures the words on
/// purpose, so it bypasses the vocabulary guard; every other mode must keep at
/// least half the speaker's words.
fn accept_output(mode: &crate::prompt::Mode, transcript: &str, text: &str) -> bool {
    matches!(mode, crate::prompt::Mode::PromptEngineer) || keeps_speaker_words(transcript, text)
}

/// Formats the transcript, falling back to the raw text on any failure so a
/// dead Ollama or an exhausted rate limit never blocks dictation.
pub async fn format(
    settings: &Settings,
    transcript: &str,
    terms: &[String],
    mode: &crate::prompt::Mode,
) -> String {
    if transcript.is_empty() {
        return String::new();
    }
    let result = match settings.formatter {
        Formatter::None => return transcript.to_string(),
        Formatter::Ollama => {
            format_ollama(&settings.ollama_model, transcript, terms, mode).await
        }
        Formatter::Groq => {
            format_groq(
                &settings.groq_api_key,
                &settings.groq_llm_model,
                transcript,
                terms,
                mode,
            )
            .await
        }
    };
    match result {
        Ok(text) if !text.is_empty() => {
            // The vocabulary guard protects the style pass from a small model
            // slipping into assistant mode. Prompt-engineer mode legitimately
            // restructures the words (imperative rewrite), so its output shares
            // little vocabulary with the transcript by design — bypass the guard
            // there, but keep the empty-output→raw fallback for every mode.
            if accept_output(mode, transcript, &text) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::{Context, Mode, Tone};

    fn as_str(v: &serde_json::Value) -> &str {
        v["content"].as_str().unwrap_or_default()
    }

    #[test]
    fn prompt_engineer_system_has_pe_prompt_and_terms_but_no_style_override() {
        let terms = vec!["utils.ts".to_string(), "React".to_string()];
        let messages = build_messages("clean up the parser", &terms, &Mode::PromptEngineer);
        let system = as_str(&messages[0]);
        assert!(
            system.starts_with(PE_SYSTEM_PROMPT),
            "PE system must start with the PE prompt text"
        );
        assert!(
            system.contains("Preserve these proper nouns exactly: utils.ts, React"),
            "proper-noun preservation line must be present when terms are given"
        );
        assert!(
            !system.contains("STYLE OVERRIDE"),
            "PE mode must never carry a style override"
        );
    }

    #[test]
    fn prompt_engineer_uses_pe_few_shot_not_style_shots() {
        let messages = build_messages("do the thing", &[], &Mode::PromptEngineer);
        let blob = messages
            .iter()
            .map(as_str)
            .collect::<Vec<_>>()
            .join("\n");
        // A PE few-shot turn (developer-flavored) is present.
        assert!(blob.contains("utils.ts"), "PE few-shot turns must be sent");
        // Style example turns must NOT leak into PE mode.
        assert!(
            !blob.contains("free for lunch tomorrow"),
            "style_shots must be absent under PE"
        );
        // Base style few-shot content must also be absent.
        assert!(!blob.contains("STYLE OVERRIDE"));
    }

    #[test]
    fn guard_bypassed_under_pe_enforced_under_style() {
        let transcript = "clean up the date parser in utils";
        // A prompt-engineer rewrite legitimately shares little vocabulary.
        let restructured = "Refactor the date-formatting function to handle nulls.";
        assert!(
            accept_output(&Mode::PromptEngineer, transcript, restructured),
            "PE mode must bypass the vocabulary guard"
        );
        assert!(
            !accept_output(&Mode::Style(None), transcript, restructured),
            "style mode must still discard low-overlap output"
        );
        // A faithful style rewrite keeps the speaker's words and is accepted.
        let faithful = "Clean up the date parser in utils.";
        assert!(accept_output(&Mode::Style(None), transcript, faithful));
    }

    #[test]
    fn style_none_messages_are_byte_identical_to_legacy_option_none() {
        // The pre-parity Option::None path produced exactly: the base system
        // prompt (identity, no terms/style), the base few-shot pairs, then the
        // user transcript.
        let mut expected =
            vec![json!({ "role": "system", "content": SYSTEM_PROMPT })];
        let base: Vec<serde_json::Value> =
            serde_json::from_str(FEW_SHOT_JSON).unwrap();
        expected.extend(base);
        expected.push(json!({ "role": "user", "content": "hello world" }));

        let actual = build_messages("hello world", &[], &Mode::Style(None));
        assert_eq!(actual, expected);
    }

    #[test]
    fn style_some_still_applies_register_and_style_shots() {
        // Sanity: the Style(Some(..)) path keeps the tone override + style shots
        // (unchanged behavior).
        let messages = build_messages(
            "hey",
            &[],
            &Mode::Style(Some((Tone::Formal, Context::Work))),
        );
        let blob = messages.iter().map(as_str).collect::<Vec<_>>().join("\n");
        assert!(blob.contains("STYLE OVERRIDE"), "style override must be present");
        assert!(
            blob.contains("free for lunch tomorrow"),
            "style_shots must be present under Style(Some)"
        );
    }
}
