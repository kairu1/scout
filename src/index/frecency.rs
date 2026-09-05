//! Frecency: one continuously decaying score per path, and the visit
//! credit that raises it.

use rusqlite::Connection;

use crate::Result;

/// Decay constant: ln 2 over seven days in seconds.
pub const LAMBDA: f64 = std::f64::consts::LN_2 / 604_800.0;

/// Cap applied at write time so a saturated score stays bounded.
pub const S_CAP: f64 = 10_000.0;

/// One credit per path per this many seconds. The window is a predicate
/// on the row's own `last_update`, so it holds across a long-lived
/// session and across two scout processes sharing the index; a
/// process-local map could do neither.
pub const CREDIT_WINDOW_SECS: i64 = 10;

/// Credit one visit at time `now`: `S = min(S·exp(-λ·Δt) + 1, cap)`, bump
/// `last_update` and `visits_total`, in one immediate transaction.
/// Returns `true` when the row was credited; `false` when it is
/// tombstoned or was credited less than `CREDIT_WINDOW_SECS` ago. A row
/// never visited has `last_update = 0` and always credits.
pub fn record_visit(conn: &Connection, candidate_id: i64, now: i64) -> Result<bool> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    let changed = tx.execute(
        "UPDATE paths
            SET S = min(S * exp(-:lambda * (:now - last_update)) + 1.0, :cap),
                last_update = :now,
                visits_total = visits_total + 1
          WHERE rowid = :id
            AND tombstoned_at IS NULL
            AND :now - last_update >= :window",
        rusqlite::named_params! {
            ":lambda": LAMBDA,
            ":now": now,
            ":cap": S_CAP,
            ":id": candidate_id,
            ":window": CREDIT_WINDOW_SECS,
        },
    )?;
    tx.commit()?;
    Ok(changed == 1)
}

/// Read-time decayed score; pure, writes nothing.
pub fn s_now(s_stored: f64, last_update: i64, now: i64) -> f64 {
    s_stored * (-LAMBDA * (now - last_update).max(0) as f64).exp()
}
