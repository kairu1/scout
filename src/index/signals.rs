//! SIGINT/SIGTERM discipline (Surgeon §1a, ADR-002 signal-hook slot).
//! Handlers flip one flag; the walker and insert loop poll it between
//! batches. Nothing is ever interrupted inside a BEGIN IMMEDIATE —
//! SQLite's own atomicity finishes or rolls back the in-flight batch.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};

static INTERRUPT: LazyLock<Arc<AtomicBool>> = LazyLock::new(|| Arc::new(AtomicBool::new(false)));

/// Register SIGINT and SIGTERM handlers. Idempotent enough for one
/// process lifetime; call once from the binary entry point.
pub fn install() -> std::io::Result<()> {
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&INTERRUPT))?;
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&INTERRUPT))?;
    Ok(())
}

pub fn interrupt_requested() -> bool {
    INTERRUPT.load(Ordering::Relaxed)
}

/// Programmatic trip, used by tests to simulate mid-walk SIGINT.
pub fn request_interrupt() {
    INTERRUPT.store(true, Ordering::Relaxed);
}

/// Reset between tests. Production code never clears an interrupt.
pub fn reset_interrupt() {
    INTERRUPT.store(false, Ordering::Relaxed);
}
