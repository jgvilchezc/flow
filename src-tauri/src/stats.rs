//! Usage insights computed over the history table.
//!
//! SQL handles the cheap aggregates (totals, per-app rollups, the calendar
//! heatmap). Streaks and the "fixes made" word-diff are computed in Rust as
//! pure, fully-testable functions so they have no wall-clock dependency — the
//! caller injects "today".

use chrono::{Duration, NaiveDate};
use rusqlite::Connection;
use std::collections::HashMap;

/// Aggregated usage statistics for the insights view.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Stats {
    pub total_words: i64,
    pub avg_wpm: f64,
    pub current_streak: i64,
    pub longest_streak: i64,
    pub fixes_made: i64,
    /// (app, words, sessions), busiest first, capped at 8 rows. NULL apps are
    /// bucketed under "Unknown" rather than dropped.
    pub per_app: Vec<(String, i64, i64)>,
    /// (local calendar day `YYYY-MM-DD`, words) for the last 365 days.
    pub heatmap: Vec<(String, i64)>,
}

/// Computes [`Stats`] over the full history table, using `today` as the anchor
/// for streak calculations (injected so the function stays deterministic).
pub fn get_stats(conn: &Connection, today: NaiveDate) -> rusqlite::Result<Stats> {
    let total_words: i64 = conn.query_row(
        "SELECT COALESCE(SUM(word_count), 0) FROM history",
        [],
        |r| r.get(0),
    )?;

    // WPM is words-per-minute over recorded audio time, NOT pipeline duration.
    let avg_wpm: f64 = conn.query_row(
        "SELECT COALESCE(
            SUM(word_count) * 60000.0 / NULLIF(SUM(recording_ms), 0), 0.0)
         FROM history",
        [],
        |r| r.get(0),
    )?;

    let per_app = {
        let mut stmt = conn.prepare(
            "SELECT COALESCE(app, 'Unknown') AS bucket,
                    SUM(word_count) AS words,
                    COUNT(*) AS sessions
             FROM history
             GROUP BY bucket
             ORDER BY words DESC
             LIMIT 8",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };

    let heatmap = {
        let mut stmt = conn.prepare(
            "SELECT date(at / 1000, 'unixepoch', 'localtime') AS day,
                    SUM(word_count) AS words
             FROM history
             WHERE at / 1000 >= strftime('%s', 'now', '-365 days')
             GROUP BY day
             ORDER BY day",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };

    // Distinct local days for streak math.
    let active_days: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT date(at / 1000, 'unixepoch', 'localtime') AS day
             FROM history ORDER BY day",
        )?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    let current_streak = current_streak(&active_days, today);
    let longest_streak = longest_streak(&active_days);

    // fixes_made: per-row word-diff between raw and formatted, summed.
    let fixes_made = {
        let mut stmt = conn.prepare("SELECT raw, formatted FROM history")?;
        let pairs = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        pairs
            .iter()
            .map(|(raw, fmt)| word_diff_count(raw, fmt))
            .sum::<i64>()
    };

    Ok(Stats {
        total_words,
        avg_wpm,
        current_streak,
        longest_streak,
        fixes_made,
        per_app,
        heatmap,
    })
}

/// Tokenizes into lowercased alphanumeric words (same family as
/// [`crate::format`] / [`crate::postprocess`]).
fn tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Deterministic word-level edit estimate between `raw` and `formatted`.
///
/// Both sides are tokenized into lowercased word multisets; the count is the
/// symmetric multiset difference halved and rounded up:
/// `(|raw ∖ fmt| + |fmt ∖ raw|) / 2`. A pure insertion or deletion of one word
/// counts as one fix; a one-word substitution (one removed, one added) also
/// counts as one.
pub(crate) fn word_diff_count(raw: &str, formatted: &str) -> i64 {
    let raw_tokens = tokens(raw);
    let fmt_tokens = tokens(formatted);

    let mut raw_counts: HashMap<&str, i64> = HashMap::new();
    for t in &raw_tokens {
        *raw_counts.entry(t.as_str()).or_default() += 1;
    }
    let mut fmt_counts: HashMap<&str, i64> = HashMap::new();
    for t in &fmt_tokens {
        *fmt_counts.entry(t.as_str()).or_default() += 1;
    }

    // |raw ∖ fmt|: words in raw beyond what formatted contains.
    let raw_only: i64 = raw_counts
        .iter()
        .map(|(w, &c)| (c - fmt_counts.get(w).copied().unwrap_or(0)).max(0))
        .sum();
    // |fmt ∖ raw|: words in formatted beyond what raw contains.
    let fmt_only: i64 = fmt_counts
        .iter()
        .map(|(w, &c)| (c - raw_counts.get(w).copied().unwrap_or(0)).max(0))
        .sum();

    // Halve and round up.
    (raw_only + fmt_only + 1) / 2
}

/// Parses a `YYYY-MM-DD` day string, skipping malformed entries.
fn parse_day(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

/// Length of the active streak ending today or yesterday.
///
/// `days` is a set of distinct active day strings (order-independent). The
/// streak counts consecutive calendar days ending at `today`; it is
/// yesterday-tolerant, so a streak isn't reported as broken just because the
/// user hasn't dictated yet today. Returns 0 when neither today nor yesterday
/// is active.
pub(crate) fn current_streak(days: &[String], today: NaiveDate) -> i64 {
    let set: std::collections::HashSet<NaiveDate> =
        days.iter().filter_map(|d| parse_day(d)).collect();

    let yesterday = today - Duration::days(1);
    // Anchor the streak at today if active, else yesterday if active, else none.
    let mut cursor = if set.contains(&today) {
        today
    } else if set.contains(&yesterday) {
        yesterday
    } else {
        return 0;
    };

    let mut count = 0i64;
    while set.contains(&cursor) {
        count += 1;
        cursor -= Duration::days(1);
    }
    count
}

/// Longest run of consecutive active calendar days anywhere in history.
/// Multiple dictations on the same day count once (input is distinct days).
pub(crate) fn longest_streak(days: &[String]) -> i64 {
    let mut sorted: Vec<NaiveDate> = days.iter().filter_map(|d| parse_day(d)).collect();
    sorted.sort_unstable();
    sorted.dedup();

    let mut best = 0i64;
    let mut run = 0i64;
    let mut prev: Option<NaiveDate> = None;
    for day in sorted {
        run = match prev {
            Some(p) if day == p + Duration::days(1) => run + 1,
            _ => 1,
        };
        best = best.max(run);
        prev = Some(day);
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn day(s: &str) -> String {
        s.to_string()
    }

    fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn row(at: i64, raw: &str, fmt: &str, words: i64, rec_ms: i64, app: Option<&str>) -> db::HistoryRow {
        db::HistoryRow {
            id: None,
            at,
            raw: raw.into(),
            formatted: fmt.into(),
            word_count: words,
            duration_ms: 99999, // intentionally large to prove WPM ignores it
            recording_ms: rec_ms,
            engine: "Local".into(),
            app: app.map(str::to_string),
        }
    }

    #[test]
    fn wpm_uses_recording_ms_not_duration() {
        let conn = db::open_in_memory().unwrap();
        // 100 words over 60_000 ms of audio = 100 WPM. duration_ms is huge and
        // must be ignored.
        db::insert_history(&conn, &row(1, "a", "a", 100, 60_000, None)).unwrap();
        let stats = get_stats(&conn, date("2026-01-01")).unwrap();
        assert!((stats.avg_wpm - 100.0).abs() < 1e-9, "avg_wpm = {}", stats.avg_wpm);
    }

    #[test]
    fn wpm_zero_minutes_is_zero() {
        let conn = db::open_in_memory().unwrap();
        db::insert_history(&conn, &row(1, "a", "a", 10, 0, None)).unwrap();
        let stats = get_stats(&conn, date("2026-01-01")).unwrap();
        assert_eq!(stats.avg_wpm, 0.0);
    }

    #[test]
    fn streak_current_with_yesterday_tolerance() {
        let today = date("2026-06-09");
        // Active yesterday and the day before, but not yet today.
        let days = vec![day("2026-06-07"), day("2026-06-08")];
        assert_eq!(current_streak(&days, today), 2);

        // Active today extends the streak.
        let days = vec![day("2026-06-07"), day("2026-06-08"), day("2026-06-09")];
        assert_eq!(current_streak(&days, today), 3);

        // Last active two days ago — streak is broken.
        let days = vec![day("2026-06-06"), day("2026-06-07")];
        assert_eq!(current_streak(&days, today), 0);
    }

    #[test]
    fn streak_longest_with_gap() {
        // Two runs: 3 days, gap, then 2 days. Longest is 3.
        let days = vec![
            day("2026-01-01"),
            day("2026-01-02"),
            day("2026-01-03"),
            day("2026-01-10"),
            day("2026-01-11"),
        ];
        assert_eq!(longest_streak(&days), 3);
    }

    #[test]
    fn streak_same_day_counts_once() {
        // Duplicate day strings must not inflate the streak.
        let days = vec![
            day("2026-01-01"),
            day("2026-01-01"),
            day("2026-01-02"),
            day("2026-01-02"),
            day("2026-01-02"),
        ];
        assert_eq!(longest_streak(&days), 2);
        assert_eq!(current_streak(&days, date("2026-01-02")), 2);
    }

    #[test]
    fn fixes_word_diff_plus() {
        // Table-driven: (raw, formatted, expected fixes).
        let cases = [
            ("hello world", "hello world", 0),       // identical
            ("hello world", "hello there world", 1), // one inserted word
            ("um hello world", "hello world", 1),    // one deleted filler word
            ("i went their", "I went there", 1),     // one-word substitution
            ("a b c", "x y z", 3),                    // three substitutions
            ("", "added words here", 2),              // 3 added -> ceil(3/2)=2
        ];
        for (raw, fmt, expected) in cases {
            assert_eq!(
                word_diff_count(raw, fmt),
                expected,
                "diff({raw:?}, {fmt:?})"
            );
        }
    }

    #[test]
    fn per_app_null_bucket_unknown() {
        let conn = db::open_in_memory().unwrap();
        db::insert_history(&conn, &row(1, "a", "a", 5, 1000, Some("Mail"))).unwrap();
        db::insert_history(&conn, &row(2, "a", "a", 3, 1000, None)).unwrap();
        db::insert_history(&conn, &row(3, "a", "a", 2, 1000, None)).unwrap();
        let stats = get_stats(&conn, date("2026-01-01")).unwrap();

        let unknown = stats
            .per_app
            .iter()
            .find(|(name, _, _)| name == "Unknown")
            .expect("NULL app must bucket under Unknown");
        assert_eq!(unknown.1, 5); // 3 + 2 words
        assert_eq!(unknown.2, 2); // 2 sessions
        let mail = stats.per_app.iter().find(|(n, _, _)| n == "Mail").unwrap();
        assert_eq!(mail.1, 5);
    }

    #[test]
    fn empty_db_zeroed_stats() {
        let conn = db::open_in_memory().unwrap();
        let stats = get_stats(&conn, date("2026-01-01")).unwrap();
        assert_eq!(stats.total_words, 0);
        assert_eq!(stats.avg_wpm, 0.0);
        assert_eq!(stats.current_streak, 0);
        assert_eq!(stats.longest_streak, 0);
        assert_eq!(stats.fixes_made, 0);
        assert!(stats.per_app.is_empty());
        assert!(stats.heatmap.is_empty());
    }
}
