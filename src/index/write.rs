//! The batched writer. About a thousand rows per `BEGIN IMMEDIATE`; the
//! generation advances only when the walk finishes cleanly; checkpoints
//! are explicit and yield to live queries. Every walk is of one root:
//! its rows carry the root's id, and only that root's rows are
//! tombstoned when it completes.
//!
//! Owns: the upsert, the generation allocation, completion (root pointer,
//! tombstones, purge, orphan sweep), the recon checks run per row, the
//! walk statistics. Refuses to know about: walking the filesystem (the
//! walker hands it items), ranking, terminals. Exposes: `batched_insert`,
//! `batched_insert_with`, `WriteOptions`, `InsertStats`, `MultiWalk`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use rusqlite::Connection;

use super::pacing;
use super::roots::{self, Root, WalkFlags};
use super::walk::{refused_at_boundary, WalkItem};
use crate::platform::signals;
use crate::platform::time::unix_now;
use crate::recon;
use crate::Result;

pub const DEFAULT_BATCH_SIZE: usize = 1000;

/// Tombstoned rows are purged after this long: 26 half-lives, by which
/// time a saturated score has decayed below one visit's worth, so
/// nothing worth keeping is lost and the table stays bounded.
pub const PURGE_AFTER_SECS: i64 = 182 * 86_400;

#[derive(Debug, Default, Clone)]
pub struct InsertStats {
    pub inserted: u64,
    pub skipped: u64,
    pub errors: u64,
    pub batches: u64,
    /// Generation this run wrote into rows.
    pub generation: i64,
    /// True iff the walk finished and the generation was advanced.
    pub completed: bool,
    /// Rows present in an earlier generation of this root that this walk
    /// did not see.
    pub tombstoned: u64,
    /// The root's directory was not there: nothing was walked and the
    /// previous generation still serves.
    pub absent: bool,
    /// Tombstoned rows old enough to be removed outright.
    pub purged: u64,
    /// Recon findings written during the walk (only with `recon: true`).
    pub findings: u64,
}

/// What a walk of every root reports: one entry per root in the order
/// walked, stopping early when a walk was cancelled.
#[derive(Debug, Default, Clone)]
pub struct MultiWalk {
    pub walks: Vec<(PathBuf, InsertStats)>,
    /// Roots that existed when the loop started.
    pub roots_total: usize,
}

impl MultiWalk {
    pub fn inserted(&self) -> u64 {
        self.walks.iter().map(|(_, s)| s.inserted).sum()
    }
    pub fn completed(&self) -> usize {
        self.walks.iter().filter(|(_, s)| s.completed).count()
    }
    /// True iff every root was walked to completion.
    pub fn all_completed(&self) -> bool {
        self.completed() == self.roots_total
    }
    /// The last generation written, if any walk completed.
    pub fn generation(&self) -> Option<i64> {
        self.walks.iter().rev().find(|(_, s)| s.completed).map(|(_, s)| s.generation)
    }
}

/// How a walk is written.
pub struct WriteOptions<'a> {
    pub batch_size: usize,
    /// The root being walked. Its id goes on every row; its generation
    /// pointer advances on completion.
    pub root: &'a Root,
    /// Run the stat-only recon checks on every path as it is written.
    pub recon: bool,
    /// Rows written so far, bumped once per batch, for a live display.
    pub progress: Option<Arc<AtomicU64>>,
}

/// Stream `paths` into the index as a plain walk of the root at `root`
/// (created if new, default flags, no recon checks), in batches of
/// `batch_size`. On clean completion, advance the generation; on
/// interrupt or error, prior batches stay durable but the partial
/// generation never becomes current.
pub fn batched_insert(
    conn: &mut Connection,
    root: &Path,
    paths: impl Iterator<Item = WalkItem>,
    batch_size: usize,
) -> Result<InsertStats> {
    let root = roots::ensure(conn, root, WalkFlags::default())?;
    batched_insert_with(
        conn,
        paths,
        &WriteOptions { batch_size, root: &root, recon: false, progress: None },
    )
}

/// `batched_insert` with the full set of options.
pub fn batched_insert_with(
    conn: &mut Connection,
    paths: impl Iterator<Item = WalkItem>,
    options: &WriteOptions<'_>,
) -> Result<InsertStats> {
    let batch_size = options.batch_size.max(1);
    let root_id = options.root.id;
    let mut stats = InsertStats::default();
    let home = crate::platform::xdg::home().unwrap_or_default();
    let recon_ctx =
        recon::checks::Context { euid: crate::platform::fs::euid(), now: unix_now(), home: &home };
    let stat_checks = recon::run::stat_checks();

    // Past the newest number any row carries, not just the newest
    // completed walk: an interrupted walk stamped rows with a number that
    // never became current, and reusing it would make those rows look
    // like part of this walk.
    let current: i64 = conn.query_row(
        "SELECT max(current_generation, COALESCE((SELECT max(scan_generation) FROM paths), 0))
           FROM run_state WHERE id = 1",
        [],
        |row| row.get(0),
    )?;
    let generation = current + 1;
    stats.generation = generation;

    conn.execute(
        "UPDATE run_state SET last_run_started_at = :now WHERE id = 1",
        rusqlite::named_params! { ":now": unix_now() },
    )?;
    conn.execute(
        "UPDATE roots SET last_walk_started_at = :now WHERE id = :id",
        rusqlite::named_params! { ":now": unix_now(), ":id": root_id },
    )?;

    let mut paths = paths.peekable();
    let mut batch: Vec<WalkItem> = Vec::with_capacity(batch_size);

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
                Some(item) => {
                    if refused_at_boundary(&item.path) {
                        stats.skipped += 1;
                        continue;
                    }
                    batch.push(item);
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
            // A row seen again in the same walk keeps the higher
            // `candidate` value: a path can arrive once as a candidate
            // and once as a recon-only entry.
            let mut stmt = tx.prepare_cached(
                "INSERT INTO paths (path, scan_generation, root_id, candidate)
                 VALUES (:path, :gen, :root, :candidate)
                 ON CONFLICT(path) DO UPDATE SET
                     candidate = CASE WHEN scan_generation = excluded.scan_generation
                                      THEN max(candidate, excluded.candidate)
                                      ELSE excluded.candidate END,
                     scan_generation = excluded.scan_generation,
                     root_id = excluded.root_id,
                     tombstoned_at = NULL
                 RETURNING rowid",
            )?;
            for item in &batch {
                let path = &item.path;
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
                    rusqlite::named_params! {
                        ":path": path_str,
                        ":gen": generation,
                        ":root": root_id,
                        ":candidate": item.candidate as i64,
                    },
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
        if let Some(progress) = &options.progress {
            progress.store(stats.inserted, Ordering::Relaxed);
        }

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

    // Advance the generation, tombstone every row of this root the walk
    // did not visit, and purge tombstones old enough to be worthless, all
    // in one transaction: a reader sees either the old state or the whole
    // new one.
    let now = unix_now();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE run_state SET
             current_generation = :gen,
             last_complete_generation = :gen,
             last_run_completed_at = :now
         WHERE id = 1",
        rusqlite::named_params! { ":gen": generation, ":now": now },
    )?;
    let root_rows = tx.execute(
        "UPDATE roots SET current_generation = :gen, last_walk_completed_at = :now WHERE id = :id",
        rusqlite::named_params! { ":gen": generation, ":now": now, ":id": root_id },
    )?;
    if root_rows == 0 {
        // The root was forgotten while this walk ran; its rows belong to
        // nobody now and would otherwise sit unserved and unpurged.
        tx.execute(
            "UPDATE paths SET tombstoned_at = :now WHERE root_id = :root AND tombstoned_at IS NULL",
            rusqlite::named_params! { ":now": now, ":root": root_id },
        )?;
        tx.commit()?;
        tracing::info!(generation, "root forgotten during the walk; rows tombstoned");
        return Ok(stats);
    }
    let tombstoned = tx.execute(
        "UPDATE paths SET tombstoned_at = :now
          WHERE root_id = :root AND scan_generation < :gen AND tombstoned_at IS NULL",
        rusqlite::named_params! { ":gen": generation, ":now": now, ":root": root_id },
    )?;
    if tombstoned > 0 {
        // A path that is gone has no mode to report; its findings go
        // now rather than lingering in every report until the purge.
        tx.execute(
            "DELETE FROM findings WHERE path_id IN
                 (SELECT rowid FROM paths WHERE root_id = :root AND tombstoned_at = :now)",
            rusqlite::named_params! { ":root": root_id, ":now": now },
        )?;
        tx.execute(
            "UPDATE paths SET worst_finding = 0 WHERE root_id = :root AND tombstoned_at = :now",
            rusqlite::named_params! { ":root": root_id, ":now": now },
        )?;
    }
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
        root = %options.root.path.display(),
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
