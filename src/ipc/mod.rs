//! Pipeline spine shared between search workers and the index writer
//! (ADR-001 §Index-writer pacing §3; Architect position §2).

use std::sync::atomic::AtomicU64;

/// Wall-clock milliseconds of the most recent query activity (GEN bump).
/// Search workers store into it from Phase 3; the index writer only loads
/// it, yielding checkpoints while `now - QUERY_ACTIVE < 500 ms`.
/// 0 means "no query has ever run".
pub static QUERY_ACTIVE: AtomicU64 = AtomicU64::new(0);

/// Wall-clock ms now, for QUERY_ACTIVE comparisons.
pub fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
