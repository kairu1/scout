//! Engagement 1 — schema + forward migration (ADR-001 §Consequences).

use rusqlite::Connection;
use scout::index::schema::{apply_migrations, schema_version};

fn fresh() -> Connection {
    Connection::open_in_memory().expect("in-memory db")
}

#[test]
fn fresh_db_migrates_to_version_1() {
    let conn = fresh();
    assert_eq!(schema_version(&conn).unwrap(), 0);
    apply_migrations(&conn).unwrap();
    assert_eq!(schema_version(&conn).unwrap(), 1);
}

#[test]
fn reapply_is_idempotent() {
    let conn = fresh();
    apply_migrations(&conn).unwrap();
    apply_migrations(&conn).unwrap();
    assert_eq!(schema_version(&conn).unwrap(), 1);
    // Single row in schema_version, single row in run_state.
    let rows: i64 = conn
        .query_row("SELECT count(*) FROM schema_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 1);
    let rows: i64 = conn
        .query_row("SELECT count(*) FROM run_state", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 1);
}

#[test]
fn paths_columns_match_adr_001() {
    let conn = fresh();
    apply_migrations(&conn).unwrap();

    // (name, declared type, notnull)
    let expected: &[(&str, &str, bool)] = &[
        ("path", "TEXT", true),
        ("S", "REAL", true),
        ("last_update", "INTEGER", true),
        ("visits_total", "INTEGER", true),
        ("scan_generation", "INTEGER", true),
        ("tombstoned_at", "INTEGER", false),
    ];

    let mut stmt = conn.prepare("PRAGMA table_info(paths)").unwrap();
    let cols: Vec<(String, String, bool)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)? != 0,
            ))
        })
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    assert_eq!(cols.len(), expected.len(), "column count");
    for (name, ty, notnull) in expected {
        let found = cols
            .iter()
            .find(|(n, _, _)| n == name)
            .unwrap_or_else(|| panic!("missing column {name}"));
        assert_eq!(&found.1, ty, "type of {name}");
        assert_eq!(found.2, *notnull, "notnull of {name}");
    }
}

#[test]
fn unique_index_on_canonical_path() {
    let conn = fresh();
    apply_migrations(&conn).unwrap();

    let unique: bool = conn
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_master
                WHERE type = 'index' AND tbl_name = 'paths' AND sql LIKE '%UNIQUE%'
            )",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(unique, "UNIQUE index on paths missing");

    // Enforcement: inserting the same canonical path twice must fail.
    conn.execute(
        "INSERT INTO paths (path, scan_generation) VALUES (:path, 1)",
        rusqlite::named_params! { ":path": "/tmp/x" },
    )
    .unwrap();
    let dup = conn.execute(
        "INSERT INTO paths (path, scan_generation) VALUES (:path, 1)",
        rusqlite::named_params! { ":path": "/tmp/x" },
    );
    assert!(dup.is_err(), "duplicate canonical path accepted");
}

#[test]
fn run_state_columns_present() {
    let conn = fresh();
    apply_migrations(&conn).unwrap();

    let (current, complete): (i64, i64) = conn
        .query_row(
            "SELECT current_generation, last_complete_generation FROM run_state WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(current, 0);
    assert_eq!(complete, 0);
}
