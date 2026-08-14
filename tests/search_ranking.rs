//! 1st Rifles — ranking + candidate scope (ADR-001).

use rusqlite::Connection;
use scout::index::schema::apply_migrations;
use scout::search::matcher::NucleoMatcher;
use scout::search::{index_state, load_candidates, search, IndexState};

// (path, S, last_update, visits_total, scan_generation, tombstoned_at)
type FixtureRow<'a> = (&'a str, f64, i64, i64, i64, Option<i64>);

fn db_with_rows(rows: &[FixtureRow]) -> Connection {
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
    let conn = db_with_rows(&[("/aa/x", 0.0, NOW, 1, 1, None), ("/bb/x", 0.0, NOW, 9, 1, None)]);
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
    let rows: Vec<(String, f64)> = (0..50).map(|i| (format!("/p/{i:02}"), i as f64)).collect();
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
    let highlighted: String = hit.match_indices.iter().map(|&i| chars[i as usize]).collect();
    assert_eq!(highlighted, "scout");
    // Zero-query results carry no highlights.
    let ranked = search(&mut matcher, &candidates, "", NOW, 10);
    assert!(ranked[0].match_indices.is_empty());
}

/// The defect this guards (ADR-001 revision 2026-08-14): querying a
/// project by name returned everything *inside* it and not the
/// directory itself. Against a real index, `/…/@service-hub` sat at
/// rank 317 while files four levels beneath it filled the first page.
///
/// Two causes, both fixed: the match term was saturated (a fixed
/// `k_match` of 100 against real scores of ~280), and nothing preferred
/// a candidate whose *own name* was the query.
#[test]
fn a_directory_named_for_the_query_outranks_its_contents() {
    let rows: Vec<FixtureRow> = vec![
        ("/w/projects/@service-hub", 0.0, NOW, 0, 1, None),
        ("/w/projects/@service-hub/service-hub-system", 0.0, NOW, 0, 1, None),
        ("/w/projects/@service-hub/service-hub-system/backend/tests", 0.0, NOW, 0, 1, None),
        ("/w/projects/@service-hub/service-hub-system/tools/see", 0.0, NOW, 0, 1, None),
        ("/w/projects/@mindmap/service-hub", 0.0, NOW, 0, 1, None),
    ];
    let conn = db_with_rows(&rows);
    let candidates = load_candidates(&conn).unwrap();
    let mut matcher = NucleoMatcher::new();
    let ranked = search(&mut matcher, &candidates, "service-hub", NOW, 10);

    let paths: Vec<&str> = ranked.iter().map(|r| r.path.as_str()).collect();

    // Both directories actually named for the query come first, in
    // either order — that is what the user asked for.
    let top_two: Vec<&str> = paths.iter().take(2).copied().collect();
    assert!(
        top_two.contains(&"/w/projects/@service-hub")
            && top_two.contains(&"/w/projects/@mindmap/service-hub"),
        "both directories named service-hub must lead; got {paths:#?}"
    );

    // And a file whose own name shares nothing with the query must not
    // outrank them merely by sitting inside a matching path.
    let see = paths.iter().position(|p| p.ends_with("/see")).expect("present");
    assert!(see >= 2, "a non-matching basename outranked the named dirs: {paths:#?}");
}

/// Ordering the fixture layer can establish: the shallow candidate
/// leads. The *calibration* itself is guarded in
/// `ranking::calibration_keeps_real_score_differences_visible`, because
/// nucleo scores these two identically on the path term — they contain
/// the same contiguous match — so no fixture pair can isolate the
/// constant. Discovered by writing the fixture guard, watching it read
/// a spread of exactly 0.00000, and not believing it.
#[test]
fn a_shallow_match_leads_an_equally_named_deep_one() {
    let rows: Vec<FixtureRow> = vec![
        ("/w/service-hub", 0.0, NOW, 0, 1, None),
        ("/w/x/service-hub/y/service-hub", 0.0, NOW, 0, 1, None),
    ];
    let conn = db_with_rows(&rows);
    let candidates = load_candidates(&conn).unwrap();
    let mut matcher = NucleoMatcher::new();
    let ranked = search(&mut matcher, &candidates, "service-hub", NOW, 10);
    assert_eq!(ranked.len(), 2);
    assert_eq!(ranked[0].path, "/w/service-hub");
}

/// Coverage: a name that *is* the query beats a name that merely starts
/// with it. Without this, `service-hub-system` outranked `service-hub`.
#[test]
fn an_exact_name_beats_a_longer_name_containing_it() {
    let rows: Vec<FixtureRow> = vec![
        ("/w/one/service-hub", 0.0, NOW, 0, 1, None),
        ("/w/two/service-hub-system-extended", 0.0, NOW, 0, 1, None),
    ];
    let conn = db_with_rows(&rows);
    let candidates = load_candidates(&conn).unwrap();
    let mut matcher = NucleoMatcher::new();
    let ranked = search(&mut matcher, &candidates, "service-hub", NOW, 10);
    assert_eq!(ranked[0].path, "/w/one/service-hub", "exact name must lead");
}
