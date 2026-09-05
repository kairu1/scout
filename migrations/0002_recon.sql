-- 0002_recon: what recon knows about the ground it indexes.
--
-- findings   one row per (path, check): the current state of a check on a
--            path, with a fingerprint of the fact behind it so an accepted
--            exception lapses when the fact changes.
-- exceptions the user's approved exceptions, keyed on path text rather
--            than rowid so a decision survives the row being purged and the
--            path coming back.
-- baseline   an opt-in record of a project's executable entry points, so
--            recon can say "this changed since you last looked".
-- paths.worst_finding  the highest unaccepted severity on the row, kept by
--            the two writers that change it (recon and accept), so the
--            picker reads one integer per candidate and never stats.
-- run_state.last_root  the tree the last `scout index` walked, so a
--            session can re-run it.
--
-- findings and baseline key on paths.rowid. SQLite cannot declare a
-- foreign key against an implicit rowid (the paths table has no INTEGER
-- PRIMARY KEY column), so there is no ON DELETE CASCADE here; the writer
-- that purges path rows sweeps orphaned findings and baselines itself in
-- the same transaction.

CREATE TABLE findings (
    path_id     INTEGER NOT NULL,
    check_name  TEXT    NOT NULL,
    severity    INTEGER NOT NULL,
    detail      TEXT    NOT NULL,
    fact        TEXT    NOT NULL,
    first_seen  INTEGER NOT NULL,
    last_seen   INTEGER NOT NULL,
    PRIMARY KEY (path_id, check_name)
);

CREATE INDEX idx_findings_severity ON findings(severity, path_id);

CREATE TABLE exceptions (
    path        TEXT    NOT NULL,
    check_name  TEXT    NOT NULL,
    fact        TEXT    NOT NULL,
    reason      TEXT    NOT NULL,
    accepted_at INTEGER NOT NULL,
    PRIMARY KEY (path, check_name)
);

CREATE TABLE baseline (
    path_id     INTEGER NOT NULL,
    file        TEXT    NOT NULL,
    sha256      TEXT    NOT NULL,
    size        INTEGER NOT NULL,
    recorded_at INTEGER NOT NULL,
    PRIMARY KEY (path_id, file)
);

ALTER TABLE paths ADD COLUMN worst_finding INTEGER NOT NULL DEFAULT 0;
ALTER TABLE run_state ADD COLUMN last_root TEXT;
ALTER TABLE run_state ADD COLUMN last_recon_at INTEGER;

CREATE INDEX idx_paths_generation ON paths(scan_generation);
