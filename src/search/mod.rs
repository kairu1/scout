//! Search sector (1st Rifles). Candidate scope, degradation states, and
//! the ranking pipeline per ADR-001. The matcher sits behind the
//! `Matcher` trait; blend constants live in `ranking`.

pub mod matcher;
pub mod ranking;

use rusqlite::Connection;

use crate::index::visit::s_now;
use crate::index::Result;
use crate::ipc;

use matcher::Matcher;

/// One candidate row from the current generation.
#[derive(Debug, Clone)]
pub struct CandidateRow {
    pub id: i64,
    pub path: String,
    pub s_stored: f64,
    pub last_update: i64,
    pub visits_total: i64,
}

/// A ranked result.
#[derive(Debug, Clone)]
pub struct Ranked {
    pub id: i64,
    pub path: String,
    pub rank: f64,
    pub s_now: f64,
    pub visits_total: i64,
}

/// Index degradation state for first-class banner rendering (ADR-001
/// §Degradation — banners, never error popups).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexState {
    /// generation 0, no rows: `no paths indexed — run 'scout index <path>'`.
    Empty,
    /// generation 0, a scan has started: serve nothing, banner progress.
    FirstScanInProgress { rows_so_far: i64 },
    /// generation >= 1: serve normally.
    Ready { generation: i64, candidates: i64 },
}

pub fn index_state(conn: &Connection) -> Result<IndexState> {
    let generation: i64 = conn.query_row(
        "SELECT current_generation FROM run_state WHERE id = 1",
        [],
        |row| row.get(0),
    )?;
    if generation >= 1 {
        let candidates: i64 = conn.query_row(
            "SELECT count(*) FROM paths
              WHERE scan_generation = :gen AND tombstoned_at IS NULL",
            rusqlite::named_params! { ":gen": generation },
            |row| row.get(0),
        )?;
        return Ok(IndexState::Ready { generation, candidates });
    }
    // Generation 0: rows on disk mean a first scan is (or was) in
    // flight; serve nothing from it (ADR-001 §Degradation).
    let rows: i64 = conn.query_row("SELECT count(*) FROM paths", [], |row| row.get(0))?;
    if rows > 0 {
        return Ok(IndexState::FirstScanInProgress { rows_so_far: rows });
    }
    Ok(IndexState::Empty)
}

/// Load the candidate set: every non-tombstoned row in the current
/// `scan_generation` (ADR-001 §Candidate scope — no project filter).
pub fn load_candidates(conn: &Connection) -> Result<Vec<CandidateRow>> {
    let generation: i64 = conn.query_row(
        "SELECT current_generation FROM run_state WHERE id = 1",
        [],
        |row| row.get(0),
    )?;
    if generation < 1 {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare_cached(
        "SELECT rowid, path, S, last_update, visits_total
           FROM paths
          WHERE scan_generation = :gen AND tombstoned_at IS NULL",
    )?;
    let rows = stmt
        .query_map(rusqlite::named_params! { ":gen": generation }, |row| {
            Ok(CandidateRow {
                id: row.get(0)?,
                path: row.get(1)?,
                s_stored: row.get(2)?,
                last_update: row.get(3)?,
                visits_total: row.get(4)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Rank `candidates` for `query`. Empty query ranks by decayed frecency
/// alone (no matcher call); non-empty blends matcher and frecency norms.
/// Registers query activity on `ipc::QUERY_ACTIVE` so the index writer
/// yields checkpoints (ADR-001 §Index-writer pacing).
pub fn search(
    matcher: &mut dyn Matcher,
    candidates: &[CandidateRow],
    query: &str,
    now: i64,
    limit: usize,
) -> Vec<Ranked> {
    ipc::QUERY_ACTIVE.store(ipc::now_ms(), std::sync::atomic::Ordering::Relaxed);
    let span = tracing::debug_span!("search.query", query_len = query.len());
    let _guard = span.enter();

    let mut ranked: Vec<Ranked> = if query.is_empty() {
        candidates
            .iter()
            .map(|c| {
                let s = s_now(c.s_stored, c.last_update, now);
                Ranked {
                    id: c.id,
                    path: c.path.clone(),
                    rank: s,
                    s_now: s,
                    visits_total: c.visits_total,
                }
            })
            .collect()
    } else {
        let mut scorer = matcher.compile(query);
        candidates
            .iter()
            .filter_map(|c| {
                scorer.score(&c.path).map(|m| {
                    // Calibration telemetry ADR-001 review chorus asked
                    // for: raw m_c per query, pre-normalisation.
                    tracing::trace!(raw_match = m, path = %c.path, "match score");
                    let s = s_now(c.s_stored, c.last_update, now);
                    Ranked {
                        id: c.id,
                        path: c.path.clone(),
                        rank: ranking::blend(m, s),
                        s_now: s,
                        visits_total: c.visits_total,
                    }
                })
            })
            .collect()
    };

    ranked.sort_by(ranking::compare);
    ranked.truncate(limit);
    ranked
}
