//! The batched writer. About a thousand rows per `BEGIN IMMEDIATE`; the
//! generation advances only when the walk finishes cleanly; checkpoints
//! are explicit and yield to live queries.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::pacing;
use super::walk::refused_at_boundary;
use crate::platform::signals;
use crate::platform::time::unix_now;
use crate::recon;
use crate::Result;

pub const DEFAULT_BATCH_SIZE: usize = 1000;

/// Tombstoned rows are purged after this long: 26 half-lives, by which
/// time a saturated score has decayed below one visit's worth, so
/// nothing worth keeping is lost and the table stays bounded.
pub const PURGE_AFTER_SECS: i64 = 182 * 86_400;

#[derive(Debug, Default)]
pub struct InsertStats {
    pub inserted: u64,
    pub skipped: u64,
    pub errors: u64,
    pub batches: u64,
    /// Generation this run wrote into rows.
    pub generation: i64,
    /// True iff the walk finished and the generation was advanced.
    pub completed: bool,
    /// Rows present in an earlier generation that this walk did not see.
    pub tombstoned: u64,
    /// Tombstoned rows old enough to be removed outright.
    pub purged: u64,
    /// Recon findings written during the walk (only with `recon: true`).
    pub findings: u64,
}

/// How a walk is written.
pub struct WriteOptions<'a> {
    pub batch_size: usize,
    /// The tree being walked, recorded as `run_state.last_root` on
    /// completion so a session can re-run it.
    pub root: Option<&'a Path>,
    /// Run the stat-only recon checks on every path as it is written.
    pub recon: bool,
}

/// Stream `paths` into the index in batches of `batch_size`, each batch
/// one transaction. On clean completion, advance `current_generation`;
/// on interrupt or error, prior batches stay durable but the partial
/// generation never becomes current.
pub fn batched_insert(
    conn: &mut Connection,
    paths: impl Iterator<Item = PathBuf>,
    batch_size: usize,
) -> Result<InsertStats> {
    batched_insert_with(conn, paths, &WriteOptions { batch_size, root: None, recon: false })
}

/// `batched_insert` with the full set of options.
pub fn batched_insert_with(
    conn: &mut Connection,
    paths: impl Iterator<Item = PathBuf>,
    options: &WriteOptions<'_>,
) -> Result<InsertStats> {
    let batch_size = options.batch_size.max(1);
    let mut stats = InsertStats::default();
    let home = crate::platform::xdg::home().unwrap_or_default();
    let recon_ctx =
        recon::checks::Context { euid: crate::platform::fs::euid(), now: unix_now(), home: &home };
    let stat_checks = recon::run::stat_checks();

    let current: i64 =
        conn.query_row("SELECT current_generation FROM run_state WHERE id = 1", [], |row| {
            row.get(0)
        })?;
    let generation = current + 1;
    stats.generation = generation;

    conn.execute(
        "UPDATE run_state SET last_run_started_at = :now WHERE id = 1",
        rusqlite::named_params! { ":now": unix_now() },
    )?;

    let mut paths = paths.peekable();
    let mut batch: Vec<PathBuf> = Vec::with_capacity(batch_size);

    while paths.peek().is_some() {
        // Interrupt is honoured at batch boundaries only; a batch in
        // flight commits or rolls back whole.
        if signals::interrupt_requested() {
            tracing::info!(batches = stats.batches, "interrupt requested; walk incomplete");
            return Ok(stats);
        }

        batch.clear();
        while batch.len() < batch_size {
            match paths.next() {
                Some(path) => {
                    if refused_at_boundary(&path) {
                        stats.skipped += 1;
                        continue;
                    }
                    batch.push(path);
                }
                None => break,
            }
        }
        if batch.is_empty() {
            continue;
        }

        let span = tracing::info_span!("index.walk.batch", rows = batch.len());
        let _guard = span.enter();
        let tx = conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO paths (path, scan_generation) VALUES (:path, :gen)
                 ON CONFLICT(path) DO UPDATE SET
                     scan_generation = excluded.scan_generation,
                     tombstoned_at = NULL
                 RETURNING rowid",
            )?;
            for path in &batch {
                let path_str = match path.to_str() {
                    Some(s) => s,
                    None => {
                        // Non-UTF-8 paths are skipped rather than lossily
                        // renamed into something the user could then act
                        // on incorrectly.
                        stats.skipped += 1;
                        continue;
                    }
                };
                match stmt.query_row(
                    rusqlite::named_params! { ":path": path_str, ":gen": generation },
                    |r| r.get::<_, i64>(0),
                ) {
                    Ok(rowid) => {
                        stats.inserted += 1;
                        if options.recon {
                            // The walker already paid a realpath per entry;
                            // this adds one lstat and writes what it finds
                            // in the same transaction as the row.
                            let found = recon::run::stat_findings(path, &recon_ctx);
                            stats.findings += found.len() as u64;
                            recon::store::store_for_path(
                                &tx,
                                rowid,
                                &found,
                                &stat_checks,
                                recon_ctx.now,
                            )?;
                        }
                    }
                    Err(err) => {
                        tracing::debug!(path = %path.display(), %err, "insert error");
                        stats.errors += 1;
                    }
                }
            }
        }
        tx.commit()?;
        stats.batches += 1;

        // Keep the WAL bounded: an explicit passive checkpoint, gated on
        // query quiet time.
        if stats.batches % 16 == 0 {
            pacing::passive_checkpoint(conn);
        }
    }

    // The walker quits early on interrupt, which can drain this iterator
    // with the walk incomplete: an empty stream is not a finished walk.
    if signals::interrupt_requested() {
        tracing::info!(batches = stats.batches, "interrupt requested; walk incomplete");
        return Ok(stats);
    }

    // Advance the generation, tombstone every row the walk did not
    // visit, and purge tombstones old enough to be worthless, all in one
    // transaction: a reader sees either the old state or the whole new one.
    let now = unix_now();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE run_state SET
             current_generation = :gen,
             last_complete_generation = :gen,
             last_run_completed_at = :now,
             last_root = COALESCE(:root, last_root)
         WHERE id = 1",
        rusqlite::named_params! {
            ":gen": generation,
            ":now": now,
            ":root": options.root.map(|r| r.display().to_string()),
        },
    )?;
    let tombstoned = tx.execute(
        "UPDATE paths SET tombstoned_at = :now
          WHERE scan_generation < :gen AND tombstoned_at IS NULL",
        rusqlite::named_params! { ":gen": generation, ":now": now },
    )?;
    let purged = tx.execute(
        "DELETE FROM paths WHERE tombstoned_at IS NOT NULL AND tombstoned_at < :cutoff",
        rusqlite::named_params! { ":cutoff": now - PURGE_AFTER_SECS },
    )?;
    if purged > 0 {
        // Findings and baselines key on rowid, and SQLite cannot cascade
        // from an implicit rowid; sweep the orphans by hand.
        tx.execute("DELETE FROM findings WHERE path_id NOT IN (SELECT rowid FROM paths)", [])?;
        tx.execute("DELETE FROM baseline WHERE path_id NOT IN (SELECT rowid FROM paths)", [])?;
    }
    tx.commit()?;
    stats.tombstoned = tombstoned as u64;
    stats.purged = purged as u64;
    stats.completed = true;
    tracing::info!(
        generation,
        inserted = stats.inserted,
        skipped = stats.skipped,
        errors = stats.errors,
        batches = stats.batches,
        tombstoned = stats.tombstoned,
        purged = stats.purged,
        "index.walk.complete"
    );
    pacing::passive_checkpoint(conn);
    Ok(stats)
}
