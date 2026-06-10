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

/// Returns a short bilingual register directive for `tone` in `context`.
///
/// Each fragment is two instruction lines (English, then Spanish) describing
/// the register, and explicitly tells the formatter to apply that register in
/// the transcript's own language (Spanish or English) — the directive text
/// itself is in English (an artifact, not user-facing copy).
pub fn style_fragment(tone: Tone, context: Context) -> &'static str {
    match (tone, context) {
        // --- Formal: full capitalization and complete punctuation. ---
        (Tone::Formal, Context::Personal) =>
            "EN: Write personal messages in a formal register: full capitalization, complete punctuation, no slang. Apply this register in the transcript's own language.\nES: Escribe mensajes personales en registro formal: mayúsculas completas, puntuación completa, sin jerga. Aplica este registro en el idioma del dictado.",
        (Tone::Formal, Context::Work) =>
            "EN: Write work messages in a formal register: full capitalization, complete punctuation, professional tone. Apply this register in the transcript's own language.\nES: Escribe mensajes de trabajo en registro formal: mayúsculas completas, puntuación completa, tono profesional. Aplica este registro en el idioma del dictado.",
        (Tone::Formal, Context::Email) =>
            "EN: Write email in a formal register: full capitalization, complete punctuation, courteous and professional. Apply this register in the transcript's own language.\nES: Escribe correos en registro formal: mayúsculas completas, puntuación completa, cortés y profesional. Aplica este registro en el idioma del dictado.",
        (Tone::Formal, Context::Other) =>
            "EN: Write general writing in a formal register: full capitalization, complete punctuation, no slang. Apply this register in the transcript's own language.\nES: Escribe en registro formal: mayúsculas completas, puntuación completa, sin jerga. Aplica este registro en el idioma del dictado.",

        // --- Casual: capitalization kept, lighter punctuation. ---
        (Tone::Casual, _) => match context {
            Context::Personal =>
                "EN: Write personal messages in a casual register: keep sentence capitalization but use lighter punctuation; a relaxed, friendly tone. Apply this register in the transcript's own language.\nES: Escribe mensajes personales en registro casual: conserva las mayúsculas de oración pero usa puntuación ligera; tono relajado y amistoso. Aplica este registro en el idioma del dictado.",
            Context::Work =>
                "EN: Write work messages in a casual register: keep sentence capitalization but use lighter punctuation; an approachable, collegial tone. Apply this register in the transcript's own language.\nES: Escribe mensajes de trabajo en registro casual: conserva las mayúsculas de oración pero usa puntuación ligera; tono cercano y colegiado. Aplica este registro en el idioma del dictado.",
            Context::Email =>
                "EN: Write email in a casual register: keep sentence capitalization but use lighter punctuation; a warm, conversational tone. Apply this register in the transcript's own language.\nES: Escribe correos en registro casual: conserva las mayúsculas de oración pero usa puntuación ligera; tono cálido y conversacional. Aplica este registro en el idioma del dictado.",
            Context::Other =>
                "EN: Write in a casual register: keep sentence capitalization but use lighter punctuation; a relaxed tone. Apply this register in the transcript's own language.\nES: Escribe en registro casual: conserva las mayúsculas de oración pero usa puntuación ligera; tono relajado. Aplica este registro en el idioma del dictado.",
        },

        // --- Very casual: no leading caps, minimal punctuation. ---
        (Tone::VeryCasual, _) => match context {
            Context::Personal =>
                "EN: Write personal messages in a very casual register: no leading capitals, minimal punctuation, chat-style. Apply this register in the transcript's own language.\nES: Escribe mensajes personales en registro muy casual: sin mayúsculas iniciales, puntuación mínima, estilo chat. Aplica este registro en el idioma del dictado.",
            Context::Work =>
                "EN: Write work messages in a very casual register: no leading capitals, minimal punctuation, chat-style. Apply this register in the transcript's own language.\nES: Escribe mensajes de trabajo en registro muy casual: sin mayúsculas iniciales, puntuación mínima, estilo chat. Aplica este registro en el idioma del dictado.",
            Context::Email =>
                "EN: Write email in a very casual register: no leading capitals, minimal punctuation, chat-style. Apply this register in the transcript's own language.\nES: Escribe correos en registro muy casual: sin mayúsculas iniciales, puntuación mínima, estilo chat. Aplica este registro en el idioma del dictado.",
            Context::Other =>
                "EN: Write in a very casual register: no leading capitals, minimal punctuation, chat-style. Apply this register in the transcript's own language.\nES: Escribe en registro muy casual: sin mayúsculas iniciales, puntuación mínima, estilo chat. Aplica este registro en el idioma del dictado.",
        },
    }
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
        // Bilingual: both instruction lines present.
        assert!(fragment.contains("EN:"));
        assert!(fragment.contains("ES:"));
        // Must instruct applying the register in the transcript's language.
        assert!(fragment.to_lowercase().contains("language") || fragment.contains("idioma"));
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
