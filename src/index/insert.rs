//! Batched insert path (Surgeon §2, ADR-001 §Index-writer pacing).
//! ~1000 rows per BEGIN IMMEDIATE; generation advances only on a
//! cleanly-finished walk; checkpoints are explicit and yield to active
//! queries.

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use rusqlite::Connection;

use super::walk::refused_at_boundary;
use super::{signals, unix_now, Result};
use crate::ipc;

pub const DEFAULT_BATCH_SIZE: usize = 1000;

/// Checkpoint only when no query registered in the last 500 ms; yield in
/// 10 ms steps up to 2 s before forcing (ADR-001 §Index-writer pacing).
const QUERY_QUIET_MS: u64 = 500;
const YIELD_STEP_MS: u64 = 10;
const YIELD_CAP_MS: u64 = 2000;

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
}

/// Stream `paths` into the index in batches of `batch_size`, each batch
/// one BEGIN IMMEDIATE. On clean completion, advance
/// `run_state.current_generation`; on interrupt or error, prior batches
/// stay durable but the partial generation never becomes current.
pub fn batched_insert(
    conn: &mut Connection,
    paths: impl Iterator<Item = PathBuf>,
    batch_size: usize,
) -> Result<InsertStats> {
    let batch_size = batch_size.max(1);
    let mut stats = InsertStats::default();

    let current: i64 = conn.query_row(
        "SELECT current_generation FROM run_state WHERE id = 1",
        [],
        |row| row.get(0),
    )?;
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
        // flight commits or rolls back whole (Surgeon §2).
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
                     tombstoned_at = NULL",
            )?;
            for path in &batch {
                let path_str = match path.to_str() {
                    Some(s) => s,
                    None => {
                        // Non-UTF-8 paths: skip rather than lossily rename
                        // a path the user could then act on incorrectly.
                        stats.skipped += 1;
                        continue;
                    }
                };
                match stmt.execute(
                    rusqlite::named_params! { ":path": path_str, ":gen": generation },
                ) {
                    Ok(_) => stats.inserted += 1,
                    Err(err) => {
                        tracing::debug!(path = %path.display(), %err, "insert error");
                        stats.errors += 1;
                    }
                }
            }
        }
        tx.commit()?;
        stats.batches += 1;

        // Keep the WAL bounded: explicit PASSIVE checkpoint, gated on
        // query quiet time.
        if stats.batches % 16 == 0 {
            passive_checkpoint(conn);
        }
    }

    // The walker quits early on interrupt, which can drain this iterator
    // with the walk incomplete — an empty stream is not a finished walk.
    if signals::interrupt_requested() {
        tracing::info!(batches = stats.batches, "interrupt requested; walk incomplete");
        return Ok(stats);
    }

    let now = unix_now();
    conn.execute(
        "UPDATE run_state SET
             current_generation = :gen,
             last_complete_generation = :gen,
             last_run_completed_at = :now
         WHERE id = 1",
        rusqlite::named_params! { ":gen": generation, ":now": now },
    )?;
    stats.completed = true;
    tracing::info!(
        generation,
        inserted = stats.inserted,
        skipped = stats.skipped,
        errors = stats.errors,
        batches = stats.batches,
        "index.walk.complete"
    );
    passive_checkpoint(conn);
    Ok(stats)
}

/// PASSIVE checkpoint that yields while a query has been active within
/// the last 500 ms, 10 ms steps, forcing after 2 s (ADR-001 pacing §2-3).
fn passive_checkpoint(conn: &Connection) {
    let mut waited = 0u64;
    loop {
        let last_query = ipc::QUERY_ACTIVE.load(Ordering::Relaxed);
        let now = ipc::now_ms();
        if last_query == 0 || now.saturating_sub(last_query) >= QUERY_QUIET_MS || waited >= YIELD_CAP_MS
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(YIELD_STEP_MS));
        waited += YIELD_STEP_MS;
    }
    if let Err(err) = conn.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |_| Ok(())) {
        tracing::debug!(%err, "passive checkpoint failed");
    }
}
