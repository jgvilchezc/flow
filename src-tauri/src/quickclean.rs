//! Rule-based "quick clean" for short dictations.
//!
//! Wispr-style dictation sends most transcripts to an LLM formatter, but a
//! short, plain utterance ("send the report tomorrow") only needs a filler
//! strip, a capital letter, and a final period — work a handful of string rules
//! can do instantly, with no network round-trip.
//!
//! [`try_quick_clean`] is deliberately *cautious*: it returns `None` (deferring
//! to the LLM) whenever the input might need real restructuring — anything long,
//! enumerated, or carrying spoken punctuation/formatting commands the LLM is
//! responsible for (see `prompts/system_prompt.txt`). When it does return
//! `Some`, it only removes unambiguous fillers and fixes capitalization and the
//! terminal period; it never reorders or rewrites the speaker's words.
//!
//! The module is pure: no I/O, no clock, no globals — every decision is a
//! function of the arguments, which keeps it exhaustively unit-testable.

/// Single-word fillers safe to drop anywhere in the sentence. These are
/// unambiguous hesitation sounds, not content words.
const ANYWHERE_FILLERS: &[&str] = &["um", "uh", "uhm", "eh", "este"];

/// Single-word fillers only dropped when they *lead* the sentence — as real
/// words ("so big", "make it like this") they carry meaning mid-sentence, so
/// removing them there would corrupt the text.
const LEADING_FILLERS: &[&str] = &["like", "so", "bueno", "pues"];

/// Two-word fillers dropped anywhere.
const MULTIWORD_FILLERS: &[[&str; 2]] = &[["you", "know"], ["o", "sea"]];

/// Spoken enumeration / list / punctuation / formatting markers. Their presence
/// means the utterance needs LLM-level restructuring, so quick-clean bails.
const MARKER_WORDS: &[&str] = &[
    // spoken enumerations
    "first", "second", "primero", "segundo", // explicit list requests
    "list", "lista", // spoken punctuation commands
    "comma", "coma", "period", "punto",
];

/// Multi-word markers (newline commands).
const MARKER_PHRASES: &[&str] = &["new line", "nueva línea", "nueva linea"];

/// Attempts a fast rule-based cleanup of `t`.
///
/// Returns `None` (defer to the LLM formatter) when quick-clean is disabled, the
/// text is empty, the word count reaches `max_words`, or the text carries any
/// list/command marker. Otherwise returns the cleaned text: fillers stripped,
/// first letter capitalized, a terminal period appended when missing.
pub fn try_quick_clean(t: &str, max_words: u32, enabled: bool) -> Option<String> {
    if !enabled {
        return None;
    }
    let trimmed = t.trim();
    if trimmed.is_empty() {
        return None;
    }

    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    // Eligibility gate on the *original* word count: longer dictations always go
    // to the LLM. `>=` makes exactly `max_words` ineligible.
    if tokens.len() as u32 >= max_words {
        return None;
    }
    if has_markers(&tokens) {
        return None;
    }

    let cleaned = clean(&tokens);
    if cleaned.is_empty() {
        // The input was nothing but fillers — let the LLM decide.
        return None;
    }
    Some(cleaned)
}

/// Normalizes a token for comparison: lowercase, outer punctuation trimmed
/// (interior apostrophes/accents kept).
fn norm(token: &str) -> String {
    token
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}

/// True if `token` is a digit-run followed by `.` or `)` — a spoken list index
/// like `1.` or `2)`.
fn is_digit_enumeration(token: &str) -> bool {
    let bytes = token.as_bytes();
    if bytes.len() < 2 {
        return false;
    }
    let last = bytes[bytes.len() - 1];
    if last != b'.' && last != b')' {
        return false;
    }
    bytes[..bytes.len() - 1].iter().all(u8::is_ascii_digit)
}

fn has_markers(tokens: &[&str]) -> bool {
    let joined = tokens
        .iter()
        .map(|t| norm(t))
        .collect::<Vec<_>>()
        .join(" ");
    if MARKER_PHRASES.iter().any(|p| joined.contains(p)) {
        return true;
    }
    tokens.iter().any(|t| {
        is_digit_enumeration(t) || {
            let n = norm(t);
            MARKER_WORDS.contains(&n.as_str())
        }
    })
}

fn is_anywhere_filler(n: &str) -> bool {
    ANYWHERE_FILLERS.contains(&n)
}

fn is_leading_filler(n: &str) -> bool {
    LEADING_FILLERS.contains(&n)
}

/// Strips fillers and fixes capitalization / terminal punctuation.
fn clean(tokens: &[&str]) -> String {
    // 1. Drop two-word fillers anywhere.
    let mut without_multi: Vec<&str> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        if i + 1 < tokens.len() {
            let a = norm(tokens[i]);
            let b = norm(tokens[i + 1]);
            if MULTIWORD_FILLERS
                .iter()
                .any(|[x, y]| a == *x && b == *y)
            {
                i += 2;
                continue;
            }
        }
        without_multi.push(tokens[i]);
        i += 1;
    }

    // 2. Strip leading fillers (leading-only + anywhere) from the front.
    let mut start = 0;
    while start < without_multi.len() {
        let n = norm(without_multi[start]);
        if is_anywhere_filler(&n) || is_leading_filler(&n) {
            start += 1;
        } else {
            break;
        }
    }

    // 3. Drop interior anywhere-fillers from what remains.
    let kept: Vec<&str> = without_multi[start..]
        .iter()
        .copied()
        .filter(|t| !is_anywhere_filler(&norm(t)))
        .collect();

    if kept.is_empty() {
        return String::new();
    }

    let joined = kept.join(" ");
    let capitalized = capitalize_first(&joined);
    if has_terminal_punctuation(&capitalized) {
        capitalized
    } else {
        format!("{capitalized}.")
    }
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn has_terminal_punctuation(s: &str) -> bool {
    matches!(s.chars().last(), Some('.' | '!' | '?' | '…'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_returns_none() {
        assert_eq!(try_quick_clean("send the report", 12, false), None);
    }

    #[test]
    fn empty_or_whitespace_returns_none() {
        assert_eq!(try_quick_clean("", 12, true), None);
        assert_eq!(try_quick_clean("   \n\t ", 12, true), None);
    }

    #[test]
    fn at_or_above_threshold_returns_none() {
        // Exactly max_words is ineligible (boundary is `>=`).
        assert_eq!(try_quick_clean("one two three four five", 5, true), None);
        // Above threshold is also ineligible.
        assert_eq!(
            try_quick_clean("one two three four five six", 5, true),
            None
        );
    }

    #[test]
    fn enumeration_markers_defer_to_llm_en() {
        assert_eq!(
            try_quick_clean("first buy milk second buy eggs", 20, true),
            None
        );
        assert_eq!(try_quick_clean("give me a list of items", 20, true), None);
    }

    #[test]
    fn enumeration_markers_defer_to_llm_es() {
        assert_eq!(
            try_quick_clean("primero comprar leche segundo pan", 20, true),
            None
        );
        assert_eq!(try_quick_clean("hazme una lista de tareas", 20, true), None);
    }

    #[test]
    fn spoken_punctuation_and_newline_markers_defer() {
        assert_eq!(try_quick_clean("send it comma then wait", 20, true), None);
        assert_eq!(try_quick_clean("write this new line and that", 20, true), None);
        assert_eq!(try_quick_clean("primera cosa coma segunda", 20, true), None);
    }

    #[test]
    fn digit_enumeration_marker_defers() {
        assert_eq!(try_quick_clean("buy 1. milk 2. eggs", 20, true), None);
    }

    #[test]
    fn short_english_fillers_are_cleaned() {
        // Leading "so", leading "um", multiword "you know", interior "uh".
        assert_eq!(
            try_quick_clean("so um send the uh report you know", 20, true),
            Some("Send the report.".to_string())
        );
    }

    #[test]
    fn short_spanish_leading_filler_is_cleaned() {
        assert_eq!(
            try_quick_clean("eh mandame el informe mañana", 12, true),
            Some("Mandame el informe mañana.".to_string())
        );
    }

    #[test]
    fn already_clean_text_is_idempotent() {
        let input = "Send the report.";
        let once = try_quick_clean(input, 12, true).unwrap();
        assert_eq!(once, "Send the report.");
        let twice = try_quick_clean(&once, 12, true).unwrap();
        assert_eq!(twice, once, "cleaning a clean sentence must be a no-op");
    }

    #[test]
    fn question_mark_terminal_is_preserved() {
        assert_eq!(
            try_quick_clean("can you send it", 12, true),
            Some("Can you send it.".to_string())
        );
        assert_eq!(
            try_quick_clean("can you send it?", 12, true),
            Some("Can you send it?".to_string())
        );
    }

    #[test]
    fn fillers_only_input_returns_none() {
        assert_eq!(try_quick_clean("um uh you know", 12, true), None);
    }
}
