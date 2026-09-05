//! `scout open-db <path>`: open (and if needed recover) an index and
//! print its vitals.

use std::path::PathBuf;

use super::logging;
use crate::{index, Error};

pub fn open_db(db_path: PathBuf) -> crate::Result<u8> {
    logging::init(logging::Sink::Stderr);
    let conn = index::open(&db_path).map_err(|err| match err {
        Error::Io { context, source } if context == "io" => {
            Error::io(format!("open {}", db_path.display()), source)
        }
        other => other,
    })?;
    let version = index::schema::schema_version(&conn).unwrap_or(0);
    let rows: i64 = conn.query_row("SELECT count(*) FROM paths", [], |r| r.get(0)).unwrap_or(0);
    let generation: i64 = conn
        .query_row("SELECT current_generation FROM run_state WHERE id = 1", [], |r| r.get(0))
        .unwrap_or(0);
    println!("{}: schema v{version}, {rows} paths, generation {generation}", db_path.display());
    index::recovery::shutdown(conn)
        .map_err(|err| Error::io("shutdown", std::io::Error::other(err.to_string())))?;
    Ok(0)
}
