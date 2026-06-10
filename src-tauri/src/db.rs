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

#[cfg(test)]
mod tests {
    use super::*;

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
}
