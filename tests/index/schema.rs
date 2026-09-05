//! Schema and forward migration.

use rusqlite::Connection;
use scout::index::schema::{apply_migrations, schema_version};

fn fresh() -> Connection {
    Connection::open_in_memory().expect("in-memory db")
}

#[test]
fn a_fresh_db_migrates_to_the_current_version() {
    let conn = fresh();
    assert_eq!(schema_version(&conn).unwrap(), 0);
    apply_migrations(&conn).unwrap();
    assert_eq!(schema_version(&conn).unwrap(), 2);
}

/// A database left at schema 1 by v0.2.x picks up migration 2 on open,
/// keeping its rows.
#[test]
fn a_version_1_db_migrates_forward_keeping_its_rows() {
    let conn = fresh();
    conn.execute_batch(include_str!("../../migrations/0001_initial.sql")).unwrap();
    conn.execute("INSERT INTO paths (path, scan_generation, visits_total) VALUES ('/p', 1, 4)", [])
        .unwrap();
    assert_eq!(schema_version(&conn).unwrap(), 1);
    apply_migrations(&conn).unwrap();
    assert_eq!(schema_version(&conn).unwrap(), 2);
    let (visits, worst): (i64, i64) = conn
        .query_row("SELECT visits_total, worst_finding FROM paths WHERE path = '/p'", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!((visits, worst), (4, 0));
}

#[test]
fn reapplying_migrations_is_idempotent() {
    let conn = fresh();
    apply_migrations(&conn).unwrap();
    apply_migrations(&conn).unwrap();
    assert_eq!(schema_version(&conn).unwrap(), 2);
    // Single row in schema_version, single row in run_state.
    let rows: i64 =
        conn.query_row("SELECT count(*) FROM schema_version", [], |r| r.get(0)).unwrap();
    assert_eq!(rows, 1);
    let rows: i64 = conn.query_row("SELECT count(*) FROM run_state", [], |r| r.get(0)).unwrap();
    assert_eq!(rows, 1);
}

#[test]
fn paths_table_has_frecency_generation_tombstone_and_finding_columns() {
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
        ("worst_finding", "INTEGER", true),
    ];

    let mut stmt = conn.prepare("PRAGMA table_info(paths)").unwrap();
    let cols: Vec<(String, String, bool)> = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)? != 0))
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
fn the_canonical_path_is_unique() {
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
fn run_state_starts_at_generation_zero() {
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

#[test]
fn recon_tables_exist_with_their_keys() {
    let conn = fresh();
    apply_migrations(&conn).unwrap();
    for table in ["findings", "exceptions", "baseline"] {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = :n)",
                rusqlite::named_params! { ":n": table },
                |r| r.get(0),
            )
            .unwrap();
        assert!(exists, "{table} missing");
    }
    // The primary keys hold: a second finding for the same (row, check) is
    // an upsert target, not a duplicate.
    conn.execute("INSERT INTO paths (path, scan_generation) VALUES ('/p', 1)", []).unwrap();
    let id = conn.last_insert_rowid();
    let insert =
        "INSERT INTO findings (path_id, check_name, severity, detail, fact, first_seen, last_seen)
                  VALUES (:id, 'suid', 3, 'd', 'f', 1, 1)";
    conn.execute(insert, rusqlite::named_params! { ":id": id }).unwrap();
    assert!(conn.execute(insert, rusqlite::named_params! { ":id": id }).is_err());
    let last_root: Option<String> =
        conn.query_row("SELECT last_root FROM run_state WHERE id = 1", [], |r| r.get(0)).unwrap();
    assert_eq!(last_root, None);
}
