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

// The query/CRUD surface is built out across the management-ui batches and is
// wired into the Tauri command layer in a later batch; until then some public
// helpers are exercised only by tests.
#![allow(dead_code)]

use rusqlite::Connection;
use std::path::Path;

/// The schema version this build expects. `migrate` walks the ladder until the
/// database reaches this version.
pub const SCHEMA_VERSION: i64 = 1;

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
        "#,
    )
}

// ---------------------------------------------------------------------------
// history
// ---------------------------------------------------------------------------

/// One dictation entry. `id` is `None` before insertion and populated with the
/// AUTOINCREMENT rowid afterwards by [`insert_history`].
#[derive(Debug, Clone, PartialEq)]
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
}

/// Inserts a history row, returning its assigned rowid.
pub fn insert_history(conn: &Connection, row: &HistoryRow) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO history
            (at, raw, formatted, word_count, duration_ms, recording_ms, engine, app)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            row.at,
            row.raw,
            row.formatted,
            row.word_count,
            row.duration_ms,
            row.recording_ms,
            row.engine,
            row.app,
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
        })
    };
    const COLS: &str =
        "id, at, raw, formatted, word_count, duration_ms, recording_ms, engine, app";

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
}
