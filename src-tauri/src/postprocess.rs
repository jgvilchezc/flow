//! Deterministic post-processing of transcripts: literal replacements and
//! snippet expansions.
//!
//! This is the rule-based pass that runs after (or instead of) the LLM
//! formatter. It is intentionally simple and fully deterministic so behaviour
//! is testable and never surprises the user:
//!
//! * Matching is **whole-word**, using the same `char::is_alphanumeric`
//!   tokenizer family as [`crate::format`]. A rule for `addr` never fires
//!   inside `address`.
//! * Matching is **case-insensitive**, and tolerant of punctuation
//!   immediately around the matched phrase (`My email.` still matches
//!   `my email`).
//! * Multi-word phrases match a consecutive run of word tokens.
//! * **Longest phrase wins** when several rules could start at the same
//!   position, so `category` beats `cat`.
//! * A **single left-to-right pass** is made per rule set: substituted text is
//!   inserted verbatim and never re-scanned, so rules cannot cascade or loop.
//! * Replacements run first (one pass), then snippets (a second pass).
//! * Empty rule sets return the input unchanged.

/// Applies replacement rules, then snippet rules, to `text`.
///
/// Each `(from, to)` pair matches `from` as a whole-word, case-insensitive,
/// punctuation-tolerant phrase and substitutes `to` verbatim. Replacements and
/// snippets are applied as two independent single passes.
pub fn apply(text: &str, replacements: &[(String, String)], snippets: &[(String, String)]) -> String {
    let after_replacements = apply_rules(text, replacements);
    apply_rules(&after_replacements, snippets)
}

/// A word token: the lowercased text plus its byte span in the source string.
struct Token {
    lower: String,
    start: usize,
    end: usize,
}

/// Tokenizes `s` into maximal runs of alphanumeric characters, recording each
/// token's lowercased form and byte span. Everything between tokens (spaces,
/// punctuation) is preserved by the caller via the spans.
fn tokenize(s: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in s.char_indices() {
        if c.is_alphanumeric() {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(begin) = start.take() {
            tokens.push(Token {
                lower: s[begin..i].to_lowercase(),
                start: begin,
                end: i,
            });
        }
    }
    if let Some(begin) = start {
        tokens.push(Token {
            lower: s[begin..].to_lowercase(),
            start: begin,
            end: s.len(),
        });
    }
    tokens
}

/// A compiled rule: its phrase split into lowercased word tokens plus the
/// verbatim replacement text.
struct Rule {
    words: Vec<String>,
    to: String,
}

/// Applies one rule set to `text` in a single left-to-right pass over the word
/// tokens. Output is built incrementally and never re-scanned.
fn apply_rules(text: &str, rules: &[(String, String)]) -> String {
    // Empty rule set is identity — and cheap.
    if rules.is_empty() {
        return text.to_string();
    }

    // Compile rules, dropping any whose phrase has no word tokens (e.g. "" or
    // pure punctuation), which could never match a whole-word run.
    let compiled: Vec<Rule> = rules
        .iter()
        .filter_map(|(from, to)| {
            let words: Vec<String> = tokenize(from).into_iter().map(|t| t.lower).collect();
            if words.is_empty() {
                None
            } else {
                Some(Rule {
                    words,
                    to: to.clone(),
                })
            }
        })
        .collect();
    if compiled.is_empty() {
        return text.to_string();
    }

    let tokens = tokenize(text);
    let mut out = String::with_capacity(text.len());
    // Byte position in `text` already copied to `out`.
    let mut cursor = 0usize;
    // Index into `tokens`.
    let mut i = 0usize;

    while i < tokens.len() {
        // Find the longest rule matching a run of tokens starting at i.
        let mut best: Option<&Rule> = None;
        for rule in &compiled {
            let len = rule.words.len();
            if i + len > tokens.len() {
                continue;
            }
            let matches = rule
                .words
                .iter()
                .enumerate()
                .all(|(k, w)| &tokens[i + k].lower == w);
            if matches {
                match best {
                    Some(b) if b.words.len() >= len => {}
                    _ => best = Some(rule),
                }
            }
        }

        if let Some(rule) = best {
            let len = rule.words.len();
            let match_start = tokens[i].start;
            let match_end = tokens[i + len - 1].end;
            // Copy any separator text preceding the matched run verbatim.
            out.push_str(&text[cursor..match_start]);
            // Insert the replacement verbatim — never re-scanned.
            out.push_str(&rule.to);
            cursor = match_end;
            i += len;
        } else {
            // No rule starts here; advance one token, leaving the surrounding
            // text to be copied when we next flush.
            i += 1;
        }
    }

    // Copy the trailing remainder of the source.
    out.push_str(&text[cursor..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    fn replace(text: &str, pairs: &[(&str, &str)]) -> String {
        apply(text, &rules(pairs), &[])
    }

    #[test]
    fn longest_match_first() {
        // "category" must win over "cat" at the same position.
        let out = replace("category", &[("cat", "dog"), ("category", "section")]);
        assert_eq!(out, "section");
    }

    #[test]
    fn no_cascade_single_pass() {
        // "a" -> "aa" must not loop, and a replacement output is not re-scanned.
        assert_eq!(replace("a", &[("a", "aa")]), "aa");
        // "cat" -> "category", and a separate "category" -> "section" rule must
        // NOT fire on the freshly-produced "category".
        let out = replace("cat", &[("cat", "category"), ("category", "section")]);
        assert_eq!(out, "category");
    }

    #[test]
    fn case_insensitive_match() {
        assert_eq!(replace("Hello WORLD", &[("hello", "hi")]), "hi WORLD");
        assert_eq!(replace("FOO bar", &[("foo", "baz")]), "baz bar");
    }

    #[test]
    fn punctuation_tolerant() {
        // Leading/trailing punctuation around the matched phrase is preserved
        // while the phrase itself is replaced.
        assert_eq!(replace("(hello)", &[("hello", "hi")]), "(hi)");
        assert_eq!(replace("hello, world!", &[("world", "earth")]), "hello, earth!");
    }

    #[test]
    fn whole_word_not_substring() {
        // "addr" must not match inside "address".
        assert_eq!(replace("address", &[("addr", "@")]), "address");
        // But the standalone word still matches.
        assert_eq!(replace("my addr here", &[("addr", "address")]), "my address here");
    }

    #[test]
    fn replacements_then_snippets_order() {
        // Replacement turns "btw" into "by the way"; a snippet on "way" then
        // fires in the second pass.
        let r = rules(&[("btw", "by the way")]);
        let s = rules(&[("way", "WAY")]);
        assert_eq!(apply("btw", &r, &s), "by the WAY");
    }

    #[test]
    fn empty_rules_identity() {
        assert_eq!(apply("untouched text.", &[], &[]), "untouched text.");
    }

    #[test]
    fn multi_word_trigger_mid_sentence() {
        // "my email" matches inside "My email." despite casing and the period.
        let out = replace("Send to My email. now", &[("my email", "jose@example.com")]);
        assert_eq!(out, "Send to jose@example.com. now");
    }

    #[test]
    fn verbatim_casing_preserved() {
        // The replacement text is inserted exactly as given, regardless of the
        // matched token's casing.
        assert_eq!(replace("github", &[("github", "GitHub")]), "GitHub");
    }
}
