-- 0003_roots: the index holds many trees.
--
-- roots      one row per tree the user asked scout to index: its path,
--            the number of its last completed walk, and how it was walked
--            (hidden entries, symlinks, the cheap recon checks) so the
--            next walk of the same tree goes the same way.
-- paths.root_id    which root a row was walked under. NULL only for rows
--            older than any root, which no reader serves until a walk
--            claims them.
-- paths.candidate  1 for a row the picker may show; 0 for a row recorded
--            only so recon can look at it (a credential file in a
--            directory walked without hidden entries).
--
-- scan_generation stays one counter across every walk; what moves to
-- the root is the pointer to its current value. A reader serves a row
-- when its generation equals its root's, so walking one tree never
-- retires another.
--
-- The one tree a 0.3 index held becomes the first root, and every row
-- is assigned to it. run_state.last_root is then redundant and dropped.

CREATE TABLE roots (
    id                     INTEGER PRIMARY KEY,
    path                   TEXT    NOT NULL UNIQUE,
    current_generation     INTEGER NOT NULL DEFAULT 0,
    hidden                 INTEGER NOT NULL DEFAULT 0,
    follow                 INTEGER NOT NULL DEFAULT 0,
    recon                  INTEGER NOT NULL DEFAULT 1,
    last_walk_started_at   INTEGER,
    last_walk_completed_at INTEGER
);

ALTER TABLE paths ADD COLUMN root_id INTEGER;
ALTER TABLE paths ADD COLUMN candidate INTEGER NOT NULL DEFAULT 1;

CREATE INDEX idx_paths_root_generation ON paths(root_id, scan_generation);

INSERT INTO roots (path, current_generation, last_walk_completed_at)
    SELECT last_root, current_generation, last_run_completed_at
      FROM run_state
     WHERE id = 1 AND last_root IS NOT NULL;

UPDATE paths SET root_id = (SELECT min(id) FROM roots)
 WHERE EXISTS (SELECT 1 FROM roots);

ALTER TABLE run_state DROP COLUMN last_root;
