//! Visit credit primitive (ADR-001 §Frecency formula, §Visit credit).

use rusqlite::Connection;

use super::{unix_now, Result};

/// Decay constant: ln 2 / 7 days in seconds (ADR-001).
pub const LAMBDA: f64 = std::f64::consts::LN_2 / 604_800.0;

/// Frecency cap at write time (ADR-001 §Bounds).
pub const S_CAP: f64 = 10_000.0;

/// Credit one visit: `S = min(S·exp(-λ·Δt) + 1, 10000)`, bump
/// `last_update` and `visits_total`, single-statement upsert inside
/// BEGIN IMMEDIATE. Tombstoned rows are never credited.
///
/// Contract: the 10-second per-path rate limit is the CALLER's job (the
/// Phase 3 action executor). This primitive credits unconditionally.
pub fn record_visit(conn: &Connection, candidate_id: i64) -> Result<bool> {
    let now = unix_now();
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    let changed = tx.execute(
        "UPDATE paths
            SET S = min(S * exp(-:lambda * (:now - last_update)) + 1.0, :cap),
                last_update = :now,
                visits_total = visits_total + 1
          WHERE rowid = :id
            AND tombstoned_at IS NULL",
        rusqlite::named_params! {
            ":lambda": LAMBDA,
            ":now": now,
            ":cap": S_CAP,
            ":id": candidate_id,
        },
    )?;
    tx.commit()?;
    Ok(changed == 1)
}

/// Read-time decayed score (pure; no write). Exposed for the Phase 3
/// ranking blend.
pub fn s_now(s_stored: f64, last_update: i64, now: i64) -> f64 {
    s_stored * (-LAMBDA * (now - last_update).max(0) as f64).exp()
}
