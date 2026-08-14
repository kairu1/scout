//! Ranking blend (ADR-001 §Ranking interaction with fuzzy-match).
//! Single point of change for the calibration constants; a re-tune is a
//! recompile, never a migration.

/// Calibration (ADR-001, revised 2026-08-14 against measured data).
struct Blend {
    /// Nucleo's raw score scales with query length — roughly 25 points
    /// per matched character — so a *fixed* `k_match` cannot serve every
    /// query. The original 100 assumed scores of 60-200; a real 11-char
    /// query over a real index produced 272-284, which `tanh` maps to a
    /// spread of 0.0018. The match signal was saturated to nothing and
    /// ordering fell through to the tie-breakers.
    k_match_per_char: f64,
    k_frec: f64,
    w_match: f64,
    w_frec: f64,
    /// Within the match term: how much of it the *basename* carries.
    /// A query is nearly always the name of the thing wanted, not a
    /// description of where it lives.
    w_base: f64,
    w_path: f64,
}

const BLEND: Blend = Blend {
    k_match_per_char: 25.0,
    k_frec: 10.0,
    w_match: 0.6,
    w_frec: 0.4,
    w_base: 0.6,
    w_path: 0.4,
};

/// Frecency saturation constant (ADR-001 §K_frec ~ two weeks of daily
/// visits). Exported as the single source so downstream calibration —
/// e.g. the TUI signal meter — derives from it instead of re-hardcoding
/// the literal.
pub const K_FREC: f64 = BLEND.k_frec;

/// Match quality for one candidate: the whole path, and the final
/// component on its own.
#[derive(Debug, Clone, Copy)]
pub struct MatchScore {
    pub path: u32,
    /// `None` when the query does not match the basename at all.
    pub base: Option<u32>,
    /// Characters in the basename. Used for coverage: a name that *is*
    /// the query earns the whole basename bonus, one three times longer
    /// earns a third of it.
    pub base_chars: usize,
}

/// Normalising constant for this query. Scales with query length
/// because the raw score does, which is the flaw the fixed constant hid.
fn k_match(query_chars: usize) -> f64 {
    (BLEND.k_match_per_char * query_chars.max(1) as f64).max(1.0)
}

/// Blended rank score for a non-empty query. Every norm is bounded
/// [0, 1) and monotonic; a late-arriving higher score never reshuffles
/// rendered rows below it (ADR-001 §streaming stability), which is why
/// normalisation stays a pure function of the candidate and the query
/// and never of the result set.
pub fn blend(score: MatchScore, s_now: f64, query_chars: usize) -> f64 {
    let k = k_match(query_chars);
    let path_norm = (score.path as f64 / k).tanh();
    // A basename that does not match at all contributes zero — which is
    // the whole point. Searching `service-hub` should rank the directory
    // named that above a file buried inside it whose own name shares
    // nothing with the query.
    // Coverage: how much of the name the query accounts for. Without
    // it `service-hub-system` outranks the directory actually named
    // `service-hub`, because a longer name matching the same substring
    // scores marginally higher. The query is the name the user has in
    // mind; a name that is exactly it should win.
    let coverage = if score.base_chars == 0 {
        0.0
    } else {
        (query_chars as f64 / score.base_chars as f64).min(1.0)
    };
    let base_norm = (score.base.unwrap_or(0) as f64 / k).tanh() * coverage;
    let match_norm = BLEND.w_base * base_norm + BLEND.w_path * path_norm;
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
