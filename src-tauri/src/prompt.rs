//! Prompt builders for STT biasing and formatter style control.
//!
//! Three additive helpers feed the dictation pipeline:
//!
//! * [`stt_initial_prompt`] biases whisper toward the user's vocabulary by
//!   seeding an `initial_prompt` of known terms.
//! * [`style_fragment`] returns a bilingual register directive for a tone in a
//!   given context, instructing the formatter to apply the register in the
//!   transcript's own language.
//! * [`augment_system_prompt`] appends term-preservation and an optional style
//!   fragment to a base system prompt without ever mutating the base.

/// Maximum length, in bytes, of the STT initial prompt. whisper truncates long
/// prompts, so the most relevant (most recently added) terms are kept.
const MAX_INITIAL_PROMPT: usize = 800;

/// Builds whisper's `initial_prompt` from known vocabulary terms.
///
/// Terms are joined with ", " and capped at [`MAX_INITIAL_PROMPT`] bytes,
/// truncating at a term boundary (never mid-term). The caller passes terms in
/// recency order (most-recently-added first), so truncation drops the oldest
/// terms. Returns `None` when there are no terms.
pub fn stt_initial_prompt(terms: &[String]) -> Option<String> {
    if terms.is_empty() {
        return None;
    }

    let mut out = String::new();
    for term in terms {
        let term = term.trim();
        if term.is_empty() {
            continue;
        }
        let addition = if out.is_empty() {
            term.to_string()
        } else {
            format!(", {term}")
        };
        if out.len() + addition.len() > MAX_INITIAL_PROMPT {
            // Adding this term would exceed the cap; stop at the last whole
            // term that fit.
            break;
        }
        out.push_str(&addition);
    }

    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// The four supported formatter tones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Formal,
    Casual,
    VeryCasual,
}

/// The four writing contexts the user can switch between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    Personal,
    Work,
    Email,
    Other,
}

/// How the formatter should rewrite the transcript.
///
/// * [`Mode::Style`] is the default dictation pass: clean the transcript and
///   apply the optional tone/context register (identical to the pre-parity
///   `Option<(Tone, Context)>` argument).
/// * [`Mode::PromptEngineer`] restructures a raw dictation into a well-formed
///   AI prompt — used for developer/AI apps that expect prompt text, not prose.
#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Style(Option<(Tone, Context)>),
    PromptEngineer,
}

/// Composes one register-override fragment from a tone header, a context noun,
/// the tone's rules, and a before→after example pair. The examples keep the
/// speaker's words identical across tones — only capitalization and
/// punctuation vary — so the `keeps_speaker_words` guard never fires on a
/// style rewrite. Small instruct models obey imperative overrides with
/// examples, not descriptive register prose (verified via bench_format.mjs).
macro_rules! style_override {
    ($tone:literal, $noun:literal, $rules:literal, $ex_en:literal, $ex_es:literal) => {
        concat!(
            "STYLE OVERRIDE — ", $tone, " register for ", $noun,
            ". When any rule above conflicts with this override, THIS OVERRIDE WINS. ",
            $rules,
            " Apply the register in the transcript's own language (Spanish stays Spanish, English stays English). Never change the speaker's words — adjust ONLY capitalization and punctuation.\n",
            "Style example: \"hey are you free for lunch tomorrow lets do twelve if that works\" -> \"", $ex_en, "\"\n",
            "Style example: \"dale nos vemos mañana tipo a las ocho en casa\" -> \"", $ex_es, "\""
        )
    };
}

macro_rules! formal {
    ($noun:literal) => {
        style_override!(
            "formal", $noun,
            "Capitalize every sentence and use complete punctuation: natural commas, question marks, and a final period on every sentence.",
            "Hey, are you free for lunch tomorrow? Let's do 12 if that works.",
            "Dale, nos vemos mañana tipo a las 8 en casa."
        )
    };
}

macro_rules! casual {
    ($noun:literal) => {
        style_override!(
            "casual", $noun,
            "Keep sentence capitalization, but lighten punctuation: skip optional commas and drop the final period (question marks stay).",
            "Hey are you free for lunch tomorrow? Let's do 12 if that works",
            "Dale nos vemos mañana tipo a las 8 en casa"
        )
    };
}

macro_rules! very_casual {
    ($noun:literal) => {
        style_override!(
            "very casual", $noun,
            "Lowercase everything except proper nouns — sentence starts included, even the first word. Keep apostrophes and question marks, skip commas, and never end with a period. Chat style.",
            "hey are you free for lunch tomorrow? let's do 12 if that works",
            "dale nos vemos mañana tipo a las 8 en casa"
        )
    };
}

/// Returns the register-override directive for `tone` in `context`.
///
/// The directive is an imperative block that explicitly takes precedence over
/// the base prompt's normalization rules and anchors the register with one
/// English and one Spanish before→after example (identical wording across
/// tones; only caps/punctuation differ). Directive text is in English (an
/// artifact, not user-facing copy).
pub fn style_fragment(tone: Tone, context: Context) -> &'static str {
    match (tone, context) {
        (Tone::Formal, Context::Personal) => formal!("personal messages"),
        (Tone::Formal, Context::Work) => formal!("work messages"),
        (Tone::Formal, Context::Email) => formal!("email"),
        (Tone::Formal, Context::Other) => formal!("general writing"),

        (Tone::Casual, Context::Personal) => casual!("personal messages"),
        (Tone::Casual, Context::Work) => casual!("work messages"),
        (Tone::Casual, Context::Email) => casual!("email"),
        (Tone::Casual, Context::Other) => casual!("general writing"),

        (Tone::VeryCasual, Context::Personal) => very_casual!("personal messages"),
        (Tone::VeryCasual, Context::Work) => very_casual!("work messages"),
        (Tone::VeryCasual, Context::Email) => very_casual!("email"),
        (Tone::VeryCasual, Context::Other) => very_casual!("general writing"),
    }
}

/// Register examples for `tone`, sent to the formatter as real user/assistant
/// turns. Small instruct models imitate few-shot answers far more reliably
/// than system-prompt prose — the base few-shot pairs all demonstrate
/// capitalized, punctuated output, so without these turns they drown out the
/// style directive (verified on gemma3:4b via bench_format.mjs). Wording is
/// identical across tones; only capitalization and punctuation differ.
pub fn style_shots(tone: Tone) -> [(&'static str, &'static str); 2] {
    const EN_IN: &str = "hey are you free for lunch tomorrow lets do twelve if that works";
    const ES_IN: &str = "dale nos vemos mañana tipo a las ocho en casa";
    match tone {
        Tone::Formal => [
            (EN_IN, "Hey, are you free for lunch tomorrow? Let's do 12 if that works."),
            (ES_IN, "Dale, nos vemos mañana tipo a las 8 en casa."),
        ],
        Tone::Casual => [
            (EN_IN, "Hey are you free for lunch tomorrow? Let's do 12 if that works"),
            (ES_IN, "Dale nos vemos mañana tipo a las 8 en casa"),
        ],
        Tone::VeryCasual => [
            (EN_IN, "hey are you free for lunch tomorrow? let's do 12 if that works"),
            (ES_IN, "dale nos vemos mañana tipo a las 8 en casa"),
        ],
    }
}

/// Deterministically re-registers example text to `tone` so every few-shot
/// answer demonstrates the active register. Without this, the base few-shot
/// pairs (all capitalized and punctuated) outweigh the tone's own example
/// turns on small models — gemma3:4b lowercased very-casual output in only
/// 1 of 3 runs until the base examples agreed with the register.
///
/// Formal is the identity: the base examples are already written formally.
pub fn apply_register(text: &str, tone: Tone) -> String {
    match tone {
        Tone::Formal => text.to_string(),
        Tone::Casual => strip_line_final_periods(text),
        Tone::VeryCasual => lowercase_sentence_starts(&strip_line_final_periods(text)),
    }
}

/// Removes a single trailing period at the end of each line ("..." and other
/// terminators are left alone) — the "less punctuation" half of the casual
/// registers.
fn strip_line_final_periods(text: &str) -> String {
    text.lines()
        .map(|line| {
            let trimmed = line.trim_end();
            if trimmed.ends_with('.') && !trimmed.ends_with("..") {
                &trimmed[..trimmed.len() - 1]
            } else {
                trimmed
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Lowercases the first alphabetic character of each sentence (line starts and
/// after sentence-ending punctuation) — the "no caps" half of very casual.
fn lowercase_sentence_starts(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at_sentence_start = true;
    for c in text.chars() {
        if at_sentence_start && c.is_alphabetic() {
            out.extend(c.to_lowercase());
            at_sentence_start = false;
        } else {
            out.push(c);
            match c {
                '.' | '!' | '?' | '\n' => at_sentence_start = true,
                _ => {
                    if !c.is_whitespace() && c != '-' {
                        at_sentence_start = false;
                    }
                }
            }
        }
    }
    out
}

/// Appends term-preservation and an optional style fragment to `base`.
///
/// The result is `base`, then the fragment (if any), then a "Preserve these
/// proper nouns exactly" line listing `terms`. This is strictly additive: the
/// base prompt is never modified, and when there are no terms and no fragment
/// the base is returned unchanged (identity).
pub fn augment_system_prompt(base: &str, terms: &[String], fragment: Option<&str>) -> String {
    let nonempty_terms: Vec<&str> = terms
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .collect();

    if nonempty_terms.is_empty() && fragment.is_none() {
        return base.to_string();
    }

    let mut out = base.to_string();
    if let Some(fragment) = fragment {
        out.push_str("\n\n");
        out.push_str(fragment);
    }
    if !nonempty_terms.is_empty() {
        out.push_str("\n\nPreserve these proper nouns exactly: ");
        out.push_str(&nonempty_terms.join(", "));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_prompt_caps_at_limit() {
        // 100 terms of "termN" — far beyond the 800-byte cap. The result must
        // be capped and truncated at a term boundary (no partial term, no
        // dangling separator).
        let terms: Vec<String> = (0..100).map(|i| format!("term{i:04}")).collect();
        let prompt = stt_initial_prompt(&terms).unwrap();
        assert!(prompt.len() <= MAX_INITIAL_PROMPT, "len = {}", prompt.len());
        assert!(!prompt.ends_with(", "), "must not end on a separator");
        // Recency order is preserved: the first (most recent) term is kept.
        assert!(prompt.starts_with("term0000"));
        // Every retained piece is a complete term.
        for piece in prompt.split(", ") {
            assert!(piece.starts_with("term") && piece.len() == 8, "partial term: {piece:?}");
        }
    }

    #[test]
    fn zero_terms_identity_prompt() {
        // No terms -> no initial prompt.
        assert_eq!(stt_initial_prompt(&[]), None);
        // No terms and no fragment -> base returned unchanged.
        let base = "You are a transcription formatter.";
        assert_eq!(augment_system_prompt(base, &[], None), base);
    }

    #[test]
    fn style_fragment_appends_without_replacing_base() {
        let base = "BASE PROMPT";
        let fragment = style_fragment(Tone::Formal, Context::Email);
        let out = augment_system_prompt(base, &[], Some(fragment));
        assert!(out.starts_with(base), "base must be preserved at the start");
        assert!(out.contains(fragment), "fragment must be appended");
        assert!(out.len() > base.len());
        // Imperative override with explicit precedence over the base rules.
        assert!(fragment.contains("OVERRIDE WINS"));
        // Anchored with both an English and a Spanish before→after example.
        assert_eq!(fragment.matches("Style example:").count(), 2);
        // Must instruct applying the register in the transcript's language.
        assert!(fragment.to_lowercase().contains("language"));
    }

    #[test]
    fn apply_register_formal_is_identity() {
        let text = "The meeting is at 5 pm. Don't be late.";
        assert_eq!(apply_register(text, Tone::Formal), text);
    }

    #[test]
    fn apply_register_casual_drops_line_final_period() {
        assert_eq!(
            apply_register("The meeting is at 5 pm. Don't be late.", Tone::Casual),
            "The meeting is at 5 pm. Don't be late"
        );
        // Multi-line: each line loses only its single trailing period.
        assert_eq!(
            apply_register("I need three things:\n- Milk.\n- Eggs.", Tone::Casual),
            "I need three things:\n- Milk\n- Eggs"
        );
    }

    #[test]
    fn apply_register_very_casual_lowercases_sentence_starts() {
        assert_eq!(
            apply_register("The meeting is at 5 pm. Don't be late.", Tone::VeryCasual),
            "the meeting is at 5 pm. don't be late"
        );
        // List items count as line starts; apostrophes survive.
        assert_eq!(
            apply_register("I need you to buy:\n- Milk\n- Eggs", Tone::VeryCasual),
            "i need you to buy:\n- milk\n- eggs"
        );
    }

    #[test]
    fn style_shots_match_their_register() {
        // very casual shots must themselves be lowercase, period-free.
        for (_, output) in style_shots(Tone::VeryCasual) {
            assert_eq!(output, apply_register(output, Tone::VeryCasual));
        }
        // casual shots carry no trailing period.
        for (_, output) in style_shots(Tone::Casual) {
            assert!(!output.ends_with('.'));
        }
    }

    #[test]
    fn proper_nouns_appended() {
        let base = "BASE";
        let terms = vec!["Tauri".to_string(), "rusqlite".to_string()];
        let out = augment_system_prompt(base, &terms, None);
        assert!(out.starts_with("BASE"));
        assert!(out.contains("Preserve these proper nouns exactly: Tauri, rusqlite"));
    }
}
