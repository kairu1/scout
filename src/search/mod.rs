//! Search: turning a query into an ordered list of candidates.
//!
//! Owns: the candidate scope (every live candidate row at its root's
//! current generation),
//! the three index states the picker renders as banners, and the ranking
//! pipeline that blends match quality with frecency.
//! Refuses to know about: the schema beyond its two SELECTs, drawing,
//! executing. The matcher sits behind a trait so it can be swapped.
//! Exposes: `index_state`, `load_candidates`, `search`, `CandidateRow`,
//! `Ranked`, `IndexState`.

pub mod matcher;
pub mod ranking;

use rusqlite::Connection;

use crate::index::frecency::s_now;
use crate::index::pacing;
use crate::Result;

use matcher::Matcher;

/// One candidate row from the current generation.
#[derive(Debug, Clone)]
pub struct CandidateRow {
    pub id: i64,
    pub path: String,
    pub s_stored: f64,
    pub last_update: i64,
    pub visits_total: i64,
    /// Highest unaccepted recon severity on the row (0 none, 1 low, 2
    /// high, 3 critical), kept current by the recon writers so the picker
    /// never stats for it.
    pub worst_finding: u8,
}

/// A ranked result.
#[derive(Debug, Clone)]
pub struct Ranked {
    pub id: i64,
    pub path: String,
    pub rank: f64,
    pub s_now: f64,
    pub visits_total: i64,
    /// Char indices the matcher matched (empty on an empty query), for
    /// highlighting.
    pub match_indices: Vec<u32>,
    pub worst_finding: u8,
}

/// What the index can serve right now. Rendered as banners, never as
/// errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexState {
    /// Generation 0, no rows: nothing has been indexed.
    Empty,
    /// Generation 0 with rows: a first scan is (or was) in flight; serve
    /// nothing from it.
    FirstScanInProgress { rows_so_far: i64 },
    /// Generation 1 or later: serve normally.
    Ready { generation: i64, candidates: i64 },
}

pub fn index_state(conn: &Connection) -> Result<IndexState> {
    let generation: i64 =
        conn.query_row("SELECT current_generation FROM run_state WHERE id = 1", [], |row| {
            row.get(0)
        })?;
    if generation >= 1 {
        let candidates: i64 = conn.query_row(
            "SELECT count(*) FROM paths p JOIN roots r ON p.root_id = r.id
              WHERE p.scan_generation = r.current_generation
                AND p.tombstoned_at IS NULL AND p.candidate = 1",
            [],
            |row| row.get(0),
        )?;
        return Ok(IndexState::Ready { generation, candidates });
    }
    let rows: i64 = conn.query_row("SELECT count(*) FROM paths", [], |row| row.get(0))?;
    if rows > 0 {
        return Ok(IndexState::FirstScanInProgress { rows_so_far: rows });
    }
    Ok(IndexState::Empty)
}

/// Every non-tombstoned candidate row at its root's current generation.
/// No project filter: the candidate set is the whole index. A row whose
/// root has been forgotten, or that predates every root, is not served.
pub fn load_candidates(conn: &Connection) -> Result<Vec<CandidateRow>> {
    let generation: i64 =
        conn.query_row("SELECT current_generation FROM run_state WHERE id = 1", [], |row| {
            row.get(0)
        })?;
    if generation < 1 {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare_cached(
        "SELECT p.rowid, p.path, p.S, p.last_update, p.visits_total, p.worst_finding
           FROM paths p JOIN roots r ON p.root_id = r.id
          WHERE p.scan_generation = r.current_generation
            AND p.tombstoned_at IS NULL AND p.candidate = 1",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(CandidateRow {
                id: row.get(0)?,
                path: row.get(1)?,
                s_stored: row.get(2)?,
                last_update: row.get(3)?,
                visits_total: row.get(4)?,
                worst_finding: row.get::<_, i64>(5)?.clamp(0, 3) as u8,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Rank `candidates` for `query`. An empty query ranks by decayed
/// frecency alone; a non-empty one blends match and frecency norms. Tells
/// the index writer a query ran, so it holds its checkpoints.
pub fn search(
    matcher: &mut dyn Matcher,
    candidates: &[CandidateRow],
    query: &str,
    now: i64,
    limit: usize,
) -> Vec<Ranked> {
    pacing::note_query_activity();
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
                    match_indices: Vec::new(),
                    worst_finding: c.worst_finding,
                }
            })
            .collect()
    } else {
        let query_chars = query.chars().count();
        let mut scorer = matcher.compile(query);
        candidates
            .iter()
            .filter_map(|c| {
                scorer.score_with_indices(&c.path).map(|(m, match_indices)| {
                    // The final path component, scored on its own: a query
                    // is nearly always the name of the thing wanted.
                    let base = c.path.rsplit('/').next().unwrap_or(&c.path);
                    let base_score = scorer.score(base);
                    tracing::trace!(
                        raw_match = m,
                        raw_base = base_score,
                        path = %c.path,
                        "match score"
                    );
                    let s = s_now(c.s_stored, c.last_update, now);
                    let score = ranking::MatchScore {
                        path: m,
                        base: base_score,
                        base_chars: base.chars().count(),
                    };
                    Ranked {
                        id: c.id,
                        path: c.path.clone(),
                        rank: ranking::blend(score, s, query_chars),
                        s_now: s,
                        visits_total: c.visits_total,
                        match_indices,
                        worst_finding: c.worst_finding,
                    }
                })
            })
            .collect()
    };

    ranked.sort_by(ranking::compare);
    ranked.truncate(limit);
    ranked
}
