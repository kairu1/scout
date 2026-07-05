//! Migrations, compile-time embedded (portability: no filesystem read at
//! runtime). Each migration runs inside one transaction; the version row
//! is advanced in the same transaction.

use rusqlite::Connection;

use super::Result;

const MIGRATIONS: &[(u32, &str)] = &[(1, include_str!("../../migrations/0001_initial.sql"))];

/// Current schema version; 0 when the database is virgin (no
/// schema_version table yet).
pub fn schema_version(conn: &Connection) -> Result<u32> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = :name)",
        rusqlite::named_params! { ":name": "schema_version" },
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(0);
    }
    let version: u32 = conn.query_row("SELECT version FROM schema_version", [], |row| row.get(0))?;
    Ok(version)
}

/// Apply every embedded migration above the database's current version,
/// in ascending order, one transaction per migration. Idempotent.
pub fn apply_migrations(conn: &Connection) -> Result<()> {
    for (version, sql) in MIGRATIONS {
        let current = schema_version(conn)?;
        if *version <= current {
            continue;
        }
        let span = tracing::info_span!("index.migrate", version = *version);
        let _guard = span.enter();
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.execute(
            "UPDATE schema_version SET version = :version",
            rusqlite::named_params! { ":version": *version },
        )?;
        tx.commit()?;
        tracing::info!(version = *version, "migration applied");
    }
    Ok(())
}
