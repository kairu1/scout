//! Ranking blend (ADR-001 §Ranking interaction with fuzzy-match).
//! Single point of change for the calibration constants; a re-tune is a
//! recompile, never a migration.

/// Calibration (ADR-001): tanh soft-normalisation, streaming-stable.
struct Blend {
    k_match: f64,
    k_frec: f64,
    w_match: f64,
    w_frec: f64,
}

const BLEND: Blend = Blend { k_match: 100.0, k_frec: 10.0, w_match: 0.6, w_frec: 0.4 };

/// Blended rank score for a non-empty query. Both norms bounded [0, 1),
/// monotonic; a late-arriving higher score never reshuffles rendered
/// rows below it.
pub fn blend(match_score: u32, s_now: f64) -> f64 {
    let match_norm = (match_score as f64 / BLEND.k_match).tanh();
    let frec_norm = (s_now.max(0.0) / BLEND.k_frec).tanh();
    BLEND.w_match * match_norm + BLEND.w_frec * frec_norm
}

/// Total order over ranked candidates (ADR-001 §Tie-breakers): rank
/// desc, then visits_total desc, shorter path, lexicographic bytes,
/// rowid asc.
pub fn compare(a: &super::Ranked, b: &super::Ranked) -> std::cmp::Ordering {
    b.rank
        .partial_cmp(&a.rank)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| b.visits_total.cmp(&a.visits_total))
        .then_with(|| a.path.len().cmp(&b.path.len()))
        .then_with(|| a.path.as_bytes().cmp(b.path.as_bytes()))
        .then_with(|| a.id.cmp(&b.id))
}
