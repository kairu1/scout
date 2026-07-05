-- 0001_initial — scout index schema, version 1.
-- Binds: ADR-001 §Consequences (paths columns, UNIQUE canonical path),
-- Surgeon §2 (run_state completion guard). One migration, whole schema.

CREATE TABLE paths (
    path            TEXT    NOT NULL,
    S               REAL    NOT NULL DEFAULT 0,
    last_update     INTEGER NOT NULL DEFAULT 0,
    visits_total    INTEGER NOT NULL DEFAULT 0,
    scan_generation INTEGER NOT NULL DEFAULT 0,
    tombstoned_at   INTEGER
);

CREATE UNIQUE INDEX idx_paths_canonical ON paths(path);

CREATE TABLE schema_version (
    version INTEGER NOT NULL
);

INSERT INTO schema_version (version) VALUES (1);

CREATE TABLE run_state (
    id                       INTEGER PRIMARY KEY CHECK (id = 1),
    current_generation       INTEGER NOT NULL DEFAULT 0,
    last_complete_generation INTEGER NOT NULL DEFAULT 0,
    last_run_started_at      INTEGER,
    last_run_completed_at    INTEGER
);

INSERT INTO run_state (id) VALUES (1);
