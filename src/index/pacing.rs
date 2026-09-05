//! Keeping the writer polite to live queries. Readers announce activity;
//! the writer checkpoints only when no query has run for a moment, and
//! never waits longer than two seconds to do so.

use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::Connection;

use crate::platform::time::now_ms;

/// Wall-clock milliseconds of the most recent query. 0 means none yet.
static QUERY_ACTIVE: AtomicU64 = AtomicU64::new(0);

/// Checkpoint only when no query registered in the last 500 ms; yield in
/// 10 ms steps up to 2 s before forcing.
const QUERY_QUIET_MS: u64 = 500;
const YIELD_STEP_MS: u64 = 10;
const YIELD_CAP_MS: u64 = 2000;

/// Called by the search path on every query.
pub fn note_query_activity() {
    QUERY_ACTIVE.store(now_ms(), Ordering::Relaxed);
}

/// Milliseconds since the epoch of the last query, or 0 if none has run.
pub fn last_query_activity_ms() -> u64 {
    QUERY_ACTIVE.load(Ordering::Relaxed)
}

/// Sleep in small steps while a query was active within the quiet
/// window, up to the cap.
pub(crate) fn wait_for_quiet() {
    let mut waited = 0u64;
    loop {
        let last_query = last_query_activity_ms();
        let now = now_ms();
        if last_query == 0
            || now.saturating_sub(last_query) >= QUERY_QUIET_MS
            || waited >= YIELD_CAP_MS
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(YIELD_STEP_MS));
        waited += YIELD_STEP_MS;
    }
}

/// A PASSIVE checkpoint that first waits for query quiet.
pub(crate) fn passive_checkpoint(conn: &Connection) {
    wait_for_quiet();
    if let Err(err) = conn.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |_| Ok(())) {
        tracing::debug!(%err, "passive checkpoint failed");
    }
}
