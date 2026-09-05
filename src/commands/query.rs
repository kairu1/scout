//! `scout query <q>`: rank and print, best first, for scripts and pipes.

use std::io::IsTerminal;

use super::{logging, open_default_db};
use crate::search::{self, IndexState};
use crate::{index, Error};

pub fn query(query: &str, limit: usize, format: &str, print0: bool) -> crate::Result<u8> {
    if !matches!(format, "paths" | "tsv") {
        return Err(Error::UnknownFormat { given: format.to_string(), wanted: "paths|tsv" });
    }
    logging::init(logging::Sink::Stderr);
    let conn = open_default_db()?;
    match search::index_state(&conn)? {
        IndexState::Empty => {
            eprintln!("no paths indexed - run 'scout index <path>' to populate");
            // Nothing matched, so exit 1: a script branching on the exit
            // code must not see "found something" on a machine with no
            // index at all.
            return Ok(1);
        }
        IndexState::FirstScanInProgress { rows_so_far } => {
            eprintln!(
                "indexing in progress ({rows_so_far} paths so far) - results will appear when \
                 the first scan completes"
            );
            return Ok(1);
        }
        IndexState::Ready { .. } => {}
    }
    let candidates = search::load_candidates(&conn)?;
    let mut matcher = search::matcher::NucleoMatcher::new();
    let now = crate::platform::time::unix_now();
    let results = search::search(&mut matcher, &candidates, query, now, limit);
    // Strip terminal escapes only when stdout is a terminal. The index
    // refuses NUL and newline in a path but permits ESC, so a directory
    // name can carry an escape sequence. Printed to a terminal it
    // rewrites the title; printed to a pipe it is data, and mangling it
    // would hand a consumer a path that does not exist on disk.
    let to_terminal = std::io::stdout().is_terminal();
    let render = |path: &str| -> String {
        if to_terminal {
            crate::ui::strip::clean(path)
        } else {
            path.to_string()
        }
    };
    for ranked in &results {
        match (format, print0) {
            // Path last in every mode: it is the only field whose bytes
            // are not under our control.
            ("tsv", _) => {
                println!("{:.4}\t{}\t{}", ranked.rank, ranked.visits_total, render(&ranked.path))
            }
            (_, true) => print!("{}\0", render(&ranked.path)),
            _ => println!("{}", render(&ranked.path)),
        }
    }
    let _ = index::recovery::shutdown(conn);
    // An exit code that never varies carries no information.
    Ok(if results.is_empty() { 1 } else { 0 })
}
