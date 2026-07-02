//! SQLite persistence layer for Flow.
//!
//! All durable application data (dictation history, the personal dictionary,
//! snippets, style configuration) lives in a single SQLite database. The schema
//! evolves through a `PRAGMA user_version` migration ladder so reopening an
//! existing database is always idempotent: each migration step runs once and
//! only when the stored version is below it.
//!
//! `open` takes an explicit path so tests can target an in-memory or temp-file
//! database while production code points at `settings::config_dir()/flow.db`.

use rusqlite::Connection;
use std::path::Path;

/// The schema version this build expects. `migrate` walks the ladder until the
/// database reaches this version.
pub const SCHEMA_VERSION: i64 = 2;

/// Opens (creating if absent) the database at `path`, enabling WAL journaling
/// and foreign-key enforcement, then runs every pending migration.
pub fn open<P: AsRef<Path>>(path: P) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

/// Opens an in-memory database (used by tests). Same pragmas and migrations as
/// [`open`].
#[cfg(test)]
pub fn open_in_memory() -> rusqlite::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

/// Connection-level pragmas. WAL keeps reads non-blocking during writes;
/// foreign-key enforcement is off by default in SQLite and must be opted into
/// per connection.
fn configure(conn: &Connection) -> rusqlite::Result<()> {
    // WAL is a no-op for `:memory:` databases but harmless to request.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

fn user_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
}

fn set_user_version(conn: &Connection, version: i64) -> rusqlite::Result<()> {
    // PRAGMA does not accept bound parameters, so the validated integer is
    // formatted directly into the statement.
    conn.execute_batch(&format!("PRAGMA user_version = {version};"))
}

/// Applies every migration whose target version is above the database's current
/// `user_version`, in ascending order. Reopening an up-to-date database is a
/// no-op.
pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let mut version = user_version(conn)?;
    if version < 1 {
        migrate_v1(conn)?;
        set_user_version(conn, 1)?;
        version = 1;
    }
    if version < 2 {
        migrate_v2(conn)?;
        set_user_version(conn, 2)?;
        version = 2;
    }
    debug_assert_eq!(version, SCHEMA_VERSION);
    Ok(())
}

/// v0 -> v1: full initial schema.
fn migrate_v1(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS history (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            at            INTEGER NOT NULL,
            raw           TEXT    NOT NULL,
            formatted     TEXT    NOT NULL,
            word_count    INTEGER NOT NULL,
            duration_ms   INTEGER NOT NULL,
            recording_ms  INTEGER NOT NULL,
            engine        TEXT    NOT NULL,
            app           TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_history_at  ON history(at DESC);
        CREATE INDEX IF NOT EXISTS idx_history_app ON history(app);

        CREATE TABLE IF NOT EXISTS dictionary (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            kind        TEXT NOT NULL CHECK(kind IN ('term','replacement')),
            phrase      TEXT NOT NULL,
            replacement TEXT,
            created_at  INTEGER NOT NULL,
            UNIQUE(kind, phrase)
        );

        CREATE TABLE IF NOT EXISTS snippets (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            trigger    TEXT NOT NULL UNIQUE,
            expansion  TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS style_config (
            context    TEXT PRIMARY KEY
                       CHECK(context IN ('personal','work','email','other')),
            tone       TEXT NOT NULL
                       CHECK(tone IN ('formal','casual','very_casual')),
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS app_meta (
            key   TEXT PRIMARY KEY,
            value TEXT
        );

        INSERT OR IGNORE INTO style_config (context, tone, updated_at) VALUES
            ('personal', 'casual', 0),
            ('work',     'casual', 0),
            ('email',    'casual', 0),
            ('other',    'casual', 0);

        INSERT OR IGNORE INTO app_meta (key, value) VALUES ('active_context', 'personal');
        "#,
    )
}

/// v1 -> v2: per-stage timing on history and the per-app formatting mode map.
///
/// The three timing columns are nullable so pre-v2 rows (and rows written
/// before the pipeline records timings) round-trip as `NULL`. `app_mode_map`
/// remembers whether a given frontmost app should be formatted with the
/// prompt-engineer or the style pipeline; it is seeded with the developer and
/// AI tools that default to prompt-engineering.
fn migrate_v2(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        ALTER TABLE history ADD COLUMN stt_ms    INTEGER;
        ALTER TABLE history ADD COLUMN format_ms INTEGER;
        ALTER TABLE history ADD COLUMN inject_ms INTEGER;

        CREATE TABLE IF NOT EXISTS app_mode_map (
            app_name   TEXT PRIMARY KEY,
            mode       TEXT NOT NULL
                       CHECK(mode IN ('prompt_engineer','style')),
            updated_at INTEGER NOT NULL
        );

        INSERT OR IGNORE INTO app_mode_map (app_name, mode, updated_at) VALUES
            ('Terminal',           'prompt_engineer', 0),
            ('iTerm2',             'prompt_engineer', 0),
            ('Warp',               'prompt_engineer', 0),
            ('Ghostty',            'prompt_engineer', 0),
            ('Visual Studio Code', 'prompt_engineer', 0),
            ('Cursor',             'prompt_engineer', 0),
            ('Windsurf',           'prompt_engineer', 0),
            ('Zed',                'prompt_engineer', 0),
            ('Xcode',              'prompt_engineer', 0),
            ('Claude',             'prompt_engineer', 0),
            ('ChatGPT',            'prompt_engineer', 0);
        "#,
    )
}

// ---------------------------------------------------------------------------
// history
// ---------------------------------------------------------------------------

/// One dictation entry. `id` is `None` before insertion and populated with the
/// AUTOINCREMENT rowid afterwards by [`insert_history`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HistoryRow {
    pub id: Option<i64>,
    /// Unix epoch milliseconds.
    pub at: i64,
    pub raw: String,
    pub formatted: String,
    pub word_count: i64,
    /// Wall-clock time spent in the STT + formatting pipeline.
    pub duration_ms: i64,
    /// Length of the captured audio (drives WPM).
    pub recording_ms: i64,
    pub engine: String,
    /// Frontmost application bundle/name, when known.
    pub app: Option<String>,
    /// Milliseconds spent in speech-to-text. `None` for pre-v2 rows.
    pub stt_ms: Option<i64>,
    /// Milliseconds spent in the formatting pass. `None` for pre-v2 rows.
    pub format_ms: Option<i64>,
    /// Milliseconds spent injecting the text. `None` for pre-v2 rows.
    pub inject_ms: Option<i64>,
}

/// Inserts a history row, returning its assigned rowid.
pub fn insert_history(conn: &Connection, row: &HistoryRow) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO history
            (at, raw, formatted, word_count, duration_ms, recording_ms, engine, app,
             stt_ms, format_ms, inject_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        rusqlite::params![
            row.at,
            row.raw,
            row.formatted,
            row.word_count,
            row.duration_ms,
            row.recording_ms,
            row.engine,
            row.app,
            row.stt_ms,
            row.format_ms,
            row.inject_ms,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Returns up to `limit` history rows newest-first. Pass the `at` of the last
/// row from the previous page as `before_at` for keyset pagination — far more
/// stable than OFFSET when new rows arrive between page loads.
pub fn get_history(
    conn: &Connection,
    limit: i64,
    before_at: Option<i64>,
) -> rusqlite::Result<Vec<HistoryRow>> {
    let map = |row: &rusqlite::Row| -> rusqlite::Result<HistoryRow> {
        Ok(HistoryRow {
            id: Some(row.get(0)?),
            at: row.get(1)?,
            raw: row.get(2)?,
            formatted: row.get(3)?,
            word_count: row.get(4)?,
            duration_ms: row.get(5)?,
            recording_ms: row.get(6)?,
            engine: row.get(7)?,
            app: row.get(8)?,
            stt_ms: row.get(9)?,
            format_ms: row.get(10)?,
            inject_ms: row.get(11)?,
        })
    };
    const COLS: &str = "id, at, raw, formatted, word_count, duration_ms, recording_ms, engine, app, \
         stt_ms, format_ms, inject_ms";

    match before_at {
        Some(before) => {
            let sql = format!(
                "SELECT {COLS} FROM history WHERE at < ?1 ORDER BY at DESC LIMIT ?2"
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt
                .query_map(rusqlite::params![before, limit], map)?
                .collect();
            rows
        }
        None => {
            let sql = format!("SELECT {COLS} FROM history ORDER BY at DESC LIMIT ?1");
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(rusqlite::params![limit], map)?.collect();
            rows
        }
    }
}

// ---------------------------------------------------------------------------
// dictionary
// ---------------------------------------------------------------------------

/// A dictionary entry. `kind` is `"term"` (a proper noun / vocabulary bias for
/// the STT prompt, `replacement` is `None`) or `"replacement"` (a literal
/// substitution where `replacement` is the target text).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct DictEntry {
    pub id: Option<i64>,
    pub kind: String,
    pub phrase: String,
    pub replacement: Option<String>,
    pub created_at: i64,
}

/// Lists dictionary entries newest-first.
pub fn list_dictionary(conn: &Connection) -> rusqlite::Result<Vec<DictEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, phrase, replacement, created_at
         FROM dictionary ORDER BY created_at DESC, id DESC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(DictEntry {
                id: Some(row.get(0)?),
                kind: row.get(1)?,
                phrase: row.get(2)?,
                replacement: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?
        .collect();
    rows
}

/// Inserts a dictionary entry, returning its rowid. The `UNIQUE(kind, phrase)`
/// constraint surfaces duplicates as a SQLite error.
pub fn add_dictionary(
    conn: &Connection,
    kind: &str,
    phrase: &str,
    replacement: Option<&str>,
    created_at: i64,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO dictionary (kind, phrase, replacement, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![kind, phrase, replacement, created_at],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Deletes a dictionary entry by id. Returns the number of rows removed.
pub fn delete_dictionary(conn: &Connection, id: i64) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM dictionary WHERE id = ?1", rusqlite::params![id])
}

// ---------------------------------------------------------------------------
// snippets
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Snippet {
    pub id: Option<i64>,
    pub trigger: String,
    pub expansion: String,
    pub created_at: i64,
}

/// Lists snippets newest-first.
pub fn list_snippets(conn: &Connection) -> rusqlite::Result<Vec<Snippet>> {
    let mut stmt = conn.prepare(
        "SELECT id, trigger, expansion, created_at
         FROM snippets ORDER BY created_at DESC, id DESC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Snippet {
                id: Some(row.get(0)?),
                trigger: row.get(1)?,
                expansion: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?
        .collect();
    rows
}

/// Inserts a snippet or updates the expansion of an existing one keyed by its
/// unique `trigger`.
pub fn upsert_snippet(
    conn: &Connection,
    trigger: &str,
    expansion: &str,
    created_at: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO snippets (trigger, expansion, created_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(trigger) DO UPDATE SET expansion = excluded.expansion",
        rusqlite::params![trigger, expansion, created_at],
    )?;
    Ok(())
}

/// Deletes a snippet by id. Returns the number of rows removed.
pub fn delete_snippet(conn: &Connection, id: i64) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM snippets WHERE id = ?1", rusqlite::params![id])
}

// ---------------------------------------------------------------------------
// style_config + active context
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct StyleContext {
    pub context: String,
    pub tone: String,
    pub updated_at: i64,
}

/// The four style contexts plus the currently active context key.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct StyleConfig {
    pub contexts: Vec<StyleContext>,
    pub active_context: String,
}

/// Returns the four seeded style rows (ordered by the canonical context order)
/// together with the active context from `app_meta`.
pub fn get_style_config(conn: &Connection) -> rusqlite::Result<StyleConfig> {
    let mut stmt = conn.prepare(
        "SELECT context, tone, updated_at FROM style_config
         ORDER BY CASE context
            WHEN 'personal' THEN 0
            WHEN 'work'     THEN 1
            WHEN 'email'    THEN 2
            WHEN 'other'    THEN 3
            ELSE 4 END",
    )?;
    let contexts: Vec<StyleContext> = stmt
        .query_map([], |row| {
            Ok(StyleContext {
                context: row.get(0)?,
                tone: row.get(1)?,
                updated_at: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    let active_context = active_context(conn)?;
    Ok(StyleConfig {
        contexts,
        active_context,
    })
}

/// Sets the tone for a context. The CHECK constraints reject invalid
/// context/tone values at the SQLite layer.
pub fn set_style(
    conn: &Connection,
    context: &str,
    tone: &str,
    updated_at: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE style_config SET tone = ?2, updated_at = ?3 WHERE context = ?1",
        rusqlite::params![context, tone, updated_at],
    )?;
    Ok(())
}

/// Reads the active context key from `app_meta`, defaulting to `personal`.
pub fn active_context(conn: &Connection) -> rusqlite::Result<String> {
    conn.query_row(
        "SELECT value FROM app_meta WHERE key = 'active_context'",
        [],
        |row| row.get::<_, Option<String>>(0),
    )
    .map(|v| v.unwrap_or_else(|| "personal".to_string()))
}

/// Sets the active context key in `app_meta`.
pub fn set_active_context(conn: &Connection, context: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO app_meta (key, value) VALUES ('active_context', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![context],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// app_mode_map
// ---------------------------------------------------------------------------

/// Lists the per-app formatting-mode overrides as `(app_name, mode)` pairs,
/// ordered by app name for a stable UI.
pub fn list_app_mode_map(conn: &Connection) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT app_name, mode FROM app_mode_map ORDER BY app_name COLLATE NOCASE",
    )?;
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect();
    rows
}

/// Upserts the formatting mode for `app_name`. The CHECK constraint rejects any
/// mode other than `prompt_engineer` or `style` at the SQLite layer.
pub fn set_app_mode(
    conn: &Connection,
    app_name: &str,
    mode: &str,
    updated_at: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO app_mode_map (app_name, mode, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(app_name) DO UPDATE SET mode = excluded.mode, updated_at = excluded.updated_at",
        rusqlite::params![app_name, mode, updated_at],
    )?;
    Ok(())
}

/// Removes the override for `app_name`. Returns the number of rows removed.
pub fn delete_app_mode(conn: &Connection, app_name: &str) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM app_mode_map WHERE app_name = ?1",
        rusqlite::params![app_name],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(at: i64, app: Option<&str>) -> HistoryRow {
        HistoryRow {
            id: None,
            at,
            raw: format!("raw {at}"),
            formatted: format!("formatted {at}"),
            word_count: 2,
            duration_ms: 100,
            recording_ms: 1000,
            engine: "Local".into(),
            app: app.map(str::to_string),
            stt_ms: None,
            format_ms: None,
            inject_ms: None,
        }
    }

    fn table_names(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        rows
    }

    #[test]
    fn opens_and_creates_schema() {
        let conn = open_in_memory().unwrap();
        let tables = table_names(&conn);
        assert!(
            tables.contains(&"history".to_string()),
            "expected history table, got {tables:?}"
        );
        assert_eq!(user_version(&conn).unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn migration_ladder_applies_in_order() {
        // A virgin connection starts at user_version 0; migrate must lift it to
        // the current schema version exactly once.
        let conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        assert_eq!(user_version(&conn).unwrap(), 0);
        migrate(&conn).unwrap();
        assert_eq!(user_version(&conn).unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn user_version_idempotent_on_reopen() {
        let dir = std::env::temp_dir().join(format!("flow-db-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("idempotent.db");
        let _ = std::fs::remove_file(&path);

        {
            let conn = open(&path).unwrap();
            assert_eq!(user_version(&conn).unwrap(), SCHEMA_VERSION);
        }
        // Reopening must not re-run migrations or change the version.
        {
            let conn = open(&path).unwrap();
            assert_eq!(user_version(&conn).unwrap(), SCHEMA_VERSION);
            assert!(table_names(&conn).contains(&"history".to_string()));
        }

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn history_insert_roundtrip() {
        let conn = open_in_memory().unwrap();
        let id = insert_history(&conn, &sample(1000, Some("Mail"))).unwrap();
        assert_eq!(id, 1);

        let rows = get_history(&conn, 10, None).unwrap();
        assert_eq!(rows.len(), 1);
        let got = &rows[0];
        assert_eq!(got.id, Some(1));
        assert_eq!(got.at, 1000);
        assert_eq!(got.raw, "raw 1000");
        assert_eq!(got.formatted, "formatted 1000");
        assert_eq!(got.word_count, 2);
        assert_eq!(got.duration_ms, 100);
        assert_eq!(got.recording_ms, 1000);
        assert_eq!(got.engine, "Local");
        assert_eq!(got.app.as_deref(), Some("Mail"));
    }

    #[test]
    fn history_keyset_pagination_before_at() {
        let conn = open_in_memory().unwrap();
        for at in [10, 20, 30, 40, 50] {
            insert_history(&conn, &sample(at, None)).unwrap();
        }
        // First page: newest two.
        let page1 = get_history(&conn, 2, None).unwrap();
        assert_eq!(page1.iter().map(|r| r.at).collect::<Vec<_>>(), vec![50, 40]);

        // Next page uses the oldest `at` from page1 as the keyset cursor.
        let cursor = page1.last().unwrap().at;
        let page2 = get_history(&conn, 2, Some(cursor)).unwrap();
        assert_eq!(page2.iter().map(|r| r.at).collect::<Vec<_>>(), vec![30, 20]);

        let cursor = page2.last().unwrap().at;
        let page3 = get_history(&conn, 2, Some(cursor)).unwrap();
        assert_eq!(page3.iter().map(|r| r.at).collect::<Vec<_>>(), vec![10]);
    }

    #[test]
    fn history_nullable_app() {
        let conn = open_in_memory().unwrap();
        insert_history(&conn, &sample(1, None)).unwrap();
        let rows = get_history(&conn, 10, None).unwrap();
        assert_eq!(rows[0].app, None);
    }

    #[test]
    fn history_survives_reopen() {
        let dir = std::env::temp_dir().join(format!("flow-db-hist-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.db");
        let _ = std::fs::remove_file(&path);

        {
            let conn = open(&path).unwrap();
            // 60 rows: more than the legacy in-memory 50 cap — the table must
            // keep all of them.
            for at in 0..60i64 {
                insert_history(&conn, &sample(at, None)).unwrap();
            }
        }
        {
            let conn = open(&path).unwrap();
            let rows = get_history(&conn, 1000, None).unwrap();
            assert_eq!(rows.len(), 60, "all 60 rows must survive reopen, no 50 cap");
        }

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dictionary_crud_unique_kind_phrase() {
        let conn = open_in_memory().unwrap();
        add_dictionary(&conn, "term", "Tauri", None, 1).unwrap();
        let id = add_dictionary(&conn, "replacement", "addr", Some("address"), 2).unwrap();

        // Same kind+phrase is rejected.
        let dup = add_dictionary(&conn, "term", "Tauri", None, 3);
        assert!(dup.is_err(), "duplicate (kind, phrase) must be rejected");

        // Same phrase under a different kind is allowed.
        add_dictionary(&conn, "replacement", "Tauri", Some("Tauri 2"), 4).unwrap();

        let rows = list_dictionary(&conn).unwrap();
        assert_eq!(rows.len(), 3);

        assert_eq!(delete_dictionary(&conn, id).unwrap(), 1);
        assert_eq!(list_dictionary(&conn).unwrap().len(), 2);
    }

    #[test]
    fn snippets_upsert_unique_trigger() {
        let conn = open_in_memory().unwrap();
        upsert_snippet(&conn, "addr", "123 Main St", 1).unwrap();
        upsert_snippet(&conn, "sig", "Best, Jose", 2).unwrap();

        // Upserting the same trigger updates rather than duplicating.
        upsert_snippet(&conn, "addr", "456 Oak Ave", 3).unwrap();

        let rows = list_snippets(&conn).unwrap();
        assert_eq!(rows.len(), 2, "upsert must not create a duplicate trigger");
        let addr = rows.iter().find(|s| s.trigger == "addr").unwrap();
        assert_eq!(addr.expansion, "456 Oak Ave");

        let id = addr.id.unwrap();
        assert_eq!(delete_snippet(&conn, id).unwrap(), 1);
        assert_eq!(list_snippets(&conn).unwrap().len(), 1);
    }

    #[test]
    fn style_four_context_rows_persist() {
        let conn = open_in_memory().unwrap();
        let cfg = get_style_config(&conn).unwrap();
        let contexts: Vec<&str> = cfg.contexts.iter().map(|c| c.context.as_str()).collect();
        assert_eq!(contexts, vec!["personal", "work", "email", "other"]);
        assert!(cfg.contexts.iter().all(|c| c.tone == "casual"));
        assert_eq!(cfg.active_context, "personal");

        set_style(&conn, "work", "formal", 99).unwrap();
        let cfg = get_style_config(&conn).unwrap();
        let work = cfg.contexts.iter().find(|c| c.context == "work").unwrap();
        assert_eq!(work.tone, "formal");
        assert_eq!(work.updated_at, 99);
        // Re-seeding on reopen must not clobber the edited tone (INSERT OR IGNORE).
        assert_eq!(cfg.contexts.len(), 4);
    }

    /// Builds a v1-shaped schema by hand (no timing columns, no app_mode_map)
    /// and stamps user_version = 1, so `migrate` exercises the v1 -> v2 step.
    fn build_v1_schema(conn: &Connection) {
        migrate_v1(conn).unwrap();
        set_user_version(conn, 1).unwrap();
        assert_eq!(user_version(conn).unwrap(), 1);
    }

    #[test]
    fn migrate_v2_is_idempotent() {
        let conn = open_in_memory().unwrap();
        assert_eq!(user_version(&conn).unwrap(), 2);
        // Running migrate again on an up-to-date DB must not move the version.
        migrate(&conn).unwrap();
        assert_eq!(user_version(&conn).unwrap(), 2);
    }

    #[test]
    fn migrate_v2_preserves_v1_rows_with_null_timings() {
        let conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        build_v1_schema(&conn);
        // A v1 history row lacks the timing columns entirely.
        conn.execute(
            "INSERT INTO history
                (at, raw, formatted, word_count, duration_ms, recording_ms, engine, app)
             VALUES (1, 'r', 'f', 1, 10, 100, 'Local', 'Mail')",
            [],
        )
        .unwrap();

        // Migrate to v2: the row must survive with NULL timings.
        migrate(&conn).unwrap();
        assert_eq!(user_version(&conn).unwrap(), SCHEMA_VERSION);

        let rows = get_history(&conn, 10, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].raw, "r");
        assert_eq!(rows[0].stt_ms, None);
        assert_eq!(rows[0].format_ms, None);
        assert_eq!(rows[0].inject_ms, None);
    }

    #[test]
    fn app_mode_map_seeds_eleven_defaults() {
        let conn = open_in_memory().unwrap();
        let map = list_app_mode_map(&conn).unwrap();
        assert_eq!(map.len(), 11, "expected 11 seeded apps, got {map:?}");
        assert!(map.iter().all(|(_, mode)| mode == "prompt_engineer"));
        let names: Vec<&str> = map.iter().map(|(n, _)| n.as_str()).collect();
        for expected in [
            "Terminal",
            "iTerm2",
            "Warp",
            "Ghostty",
            "Visual Studio Code",
            "Cursor",
            "Windsurf",
            "Zed",
            "Xcode",
            "Claude",
            "ChatGPT",
        ] {
            assert!(names.contains(&expected), "missing seed: {expected}");
        }
    }

    #[test]
    fn history_timing_columns_roundtrip() {
        let conn = open_in_memory().unwrap();
        let mut row = sample(1, Some("Warp"));
        row.stt_ms = Some(120);
        row.format_ms = Some(80);
        row.inject_ms = Some(15);
        insert_history(&conn, &row).unwrap();

        let got = &get_history(&conn, 10, None).unwrap()[0];
        assert_eq!(got.stt_ms, Some(120));
        assert_eq!(got.format_ms, Some(80));
        assert_eq!(got.inject_ms, Some(15));
    }

    #[test]
    fn app_mode_map_check_rejects_invalid_mode() {
        let conn = open_in_memory().unwrap();
        assert!(
            set_app_mode(&conn, "Slack", "bogus", 1).is_err(),
            "CHECK must reject an unknown mode"
        );
        set_app_mode(&conn, "Slack", "style", 2).unwrap();
        set_app_mode(&conn, "Slack", "prompt_engineer", 3).unwrap();
        let slack = list_app_mode_map(&conn)
            .unwrap()
            .into_iter()
            .find(|(n, _)| n == "Slack")
            .unwrap();
        assert_eq!(slack.1, "prompt_engineer", "upsert must overwrite the mode");
        assert_eq!(delete_app_mode(&conn, "Slack").unwrap(), 1);
    }

    #[test]
    fn style_active_context_toggle() {
        let conn = open_in_memory().unwrap();
        assert_eq!(active_context(&conn).unwrap(), "personal");
        set_active_context(&conn, "work").unwrap();
        assert_eq!(active_context(&conn).unwrap(), "work");
        assert_eq!(get_style_config(&conn).unwrap().active_context, "work");
        set_active_context(&conn, "email").unwrap();
        assert_eq!(active_context(&conn).unwrap(), "email");
    }
}
