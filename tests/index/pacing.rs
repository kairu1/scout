//! The writer's pacing input: readers announce activity, the writer reads
//! it. Nothing else in this binary runs a search, so the initial value is
//! observable.

use scout::index::pacing;
use scout::platform::time::now_ms;

#[test]
fn writer_pacing_sees_query_activity() {
    assert_eq!(pacing::last_query_activity_ms(), 0, "no query has run in this process yet");
    assert!(now_ms() > 0);
    pacing::note_query_activity();
    let seen = pacing::last_query_activity_ms();
    assert!(seen > 0 && seen <= now_ms(), "activity stamped with the wall clock: {seen}");
}
