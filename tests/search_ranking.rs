//! 1st Rifles — ranking + candidate scope (ADR-001).

use rusqlite::Connection;
use scout::index::schema::apply_migrations;
use scout::search::matcher::NucleoMatcher;
use scout::search::{index_state, load_candidates, search, IndexState};

fn db_with_rows(rows: &[(&str, f64, i64, i64, i64, Option<i64>)]) -> Connection {
    // (path, S, last_update, visits_total, scan_generation, tombstoned_at)
    let conn = Connection::open_in_memory().unwrap();
    apply_migrations(&conn).unwrap();
    let max_gen = rows.iter().map(|r| r.4).max().unwrap_or(0);
    conn.execute(
        "UPDATE run_state SET current_generation = :gen, last_complete_generation = :gen",
        rusqlite::named_params! { ":gen": max_gen },
    )
    .unwrap();
    for (path, s, last, visits, generation, tomb) in rows {
        conn.execute(
            "INSERT INTO paths (path, S, last_update, visits_total, scan_generation, tombstoned_at)
             VALUES (:path, :s, :last, :visits, :gen, :tomb)",
            rusqlite::named_params! {
                ":path": path, ":s": s, ":last": last,
                ":visits": visits, ":gen": generation, ":tomb": tomb,
            },
        )
        .unwrap();
    }
    conn
}

const NOW: i64 = 1_800_000_000;

#[test]
fn candidate_scope_filters_generation_and_tombstones() {
    let conn = db_with_rows(&[
        ("/a/current", 0.0, NOW, 0, 2, None),
        ("/b/stale-generation", 0.0, NOW, 0, 1, None),
        ("/c/tombstoned", 0.0, NOW, 0, 2, Some(NOW)),
    ]);
    let candidates = load_candidates(&conn).unwrap();
    let paths: Vec<_> = candidates.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(paths, vec!["/a/current"]);
}

#[test]
fn zero_query_ranks_by_decayed_frecency() {
    // hot: high S, recent. cold: high S, five half-lives old (decays to
    // ~3 %). low: small S, recent.
    let conn = db_with_rows(&[
        ("/cold", 100.0, NOW - 5 * 604_800, 50, 1, None),
        ("/hot", 100.0, NOW, 50, 1, None),
        ("/low", 1.0, NOW, 1, 1, None),
    ]);
    let candidates = load_candidates(&conn).unwrap();
    let mut matcher = NucleoMatcher::new();
    let ranked = search(&mut matcher, &candidates, "", NOW, 10);
    let paths: Vec<_> = ranked.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(paths, vec!["/hot", "/cold", "/low"]);
    assert!(ranked[0].s_now > ranked[1].s_now);
}

#[test]
fn query_eliminates_non_matches_and_blends_frecency() {
    let conn = db_with_rows(&[
        ("/home/user/projects/servicehub", 0.0, NOW, 0, 1, None),
        ("/home/user/projects/scout", 0.0, NOW, 0, 1, None),
        ("/home/user/music/album", 0.0, NOW, 0, 1, None),
    ]);
    let candidates = load_candidates(&conn).unwrap();
    let mut matcher = NucleoMatcher::new();

    let ranked = search(&mut matcher, &candidates, "scout", NOW, 10);
    let paths: Vec<_> = ranked.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(paths, vec!["/home/user/projects/scout"], "non-matches must be eliminated");

    // Same match quality, different frecency: habit breaks the tie.
    let conn = db_with_rows(&[
        ("/one/proj-a", 0.0, NOW, 0, 1, None),
        ("/two/proj-a", 50.0, NOW, 10, 1, None),
    ]);
    let candidates = load_candidates(&conn).unwrap();
    let ranked = search(&mut matcher, &candidates, "proj-a", NOW, 10);
    assert_eq!(ranked[0].path, "/two/proj-a", "frecency must lift equal matches");
}

#[test]
fn tie_breakers_are_total_and_ordered() {
    // Identical rank inputs; visits_total differs.
    let conn = db_with_rows(&[
        ("/aa/x", 0.0, NOW, 1, 1, None),
        ("/bb/x", 0.0, NOW, 9, 1, None),
    ]);
    let candidates = load_candidates(&conn).unwrap();
    let mut matcher = NucleoMatcher::new();
    let ranked = search(&mut matcher, &candidates, "", NOW, 10);
    assert_eq!(ranked[0].path, "/bb/x", "higher visits_total wins ties");

    // visits equal → shorter path wins.
    let conn = db_with_rows(&[
        ("/deep/nested/dir", 0.0, NOW, 0, 1, None),
        ("/deep", 0.0, NOW, 0, 1, None),
    ]);
    let candidates = load_candidates(&conn).unwrap();
    let ranked = search(&mut matcher, &candidates, "", NOW, 10);
    assert_eq!(ranked[0].path, "/deep", "shorter path wins ties");

    // Full equality except spelling → lexicographic.
    let conn = db_with_rows(&[("/b", 0.0, NOW, 0, 1, None), ("/a", 0.0, NOW, 0, 1, None)]);
    let candidates = load_candidates(&conn).unwrap();
    let ranked = search(&mut matcher, &candidates, "", NOW, 10);
    assert_eq!(ranked[0].path, "/a", "lexicographic byte order breaks final ties");
}

#[test]
fn degradation_states() {
    let conn = db_with_rows(&[]);
    assert_eq!(index_state(&conn).unwrap(), IndexState::Empty);

    // Generation 0 with rows on disk = first scan in flight; serve nothing.
    let conn = db_with_rows(&[("/partial", 0.0, NOW, 0, 1, None)]);
    conn.execute("UPDATE run_state SET current_generation = 0, last_complete_generation = 0", [])
        .unwrap();
    assert_eq!(index_state(&conn).unwrap(), IndexState::FirstScanInProgress { rows_so_far: 1 });
    assert!(load_candidates(&conn).unwrap().is_empty(), "partial first scan must serve nothing");

    let conn = db_with_rows(&[("/ready", 0.0, NOW, 0, 3, None)]);
    assert_eq!(index_state(&conn).unwrap(), IndexState::Ready { generation: 3, candidates: 1 });
}

#[test]
fn limit_truncates() {
    let rows: Vec<(String, f64)> =
        (0..50).map(|i| (format!("/p/{i:02}"), i as f64)).collect();
    let conn = Connection::open_in_memory().unwrap();
    apply_migrations(&conn).unwrap();
    conn.execute("UPDATE run_state SET current_generation = 1", []).unwrap();
    for (path, s) in &rows {
        conn.execute(
            "INSERT INTO paths (path, S, last_update, scan_generation) VALUES (:p, :s, :now, 1)",
            rusqlite::named_params! { ":p": path, ":s": s, ":now": NOW },
        )
        .unwrap();
    }
    let candidates = load_candidates(&conn).unwrap();
    let mut matcher = NucleoMatcher::new();
    let ranked = search(&mut matcher, &candidates, "", NOW, 7);
    assert_eq!(ranked.len(), 7);
    assert_eq!(ranked[0].path, "/p/49", "highest S first");
}

#[test]
fn match_indices_cover_query_chars() {
    let conn = db_with_rows(&[("/home/user/projects/scout", 0.0, NOW, 0, 1, None)]);
    let candidates = load_candidates(&conn).unwrap();
    let mut matcher = NucleoMatcher::new();
    let ranked = search(&mut matcher, &candidates, "scout", NOW, 10);
    let hit = &ranked[0];
    assert_eq!(hit.match_indices.len(), 5, "five query chars, five highlight positions");
    let chars: Vec<char> = hit.path.chars().collect();
    let highlighted: String =
        hit.match_indices.iter().map(|&i| chars[i as usize]).collect();
    assert_eq!(highlighted, "scout");
    // Zero-query results carry no highlights.
    let ranked = search(&mut matcher, &candidates, "", NOW, 10);
    assert!(ranked[0].match_indices.is_empty());
}
