//! What each subcommand does, as plain functions.
//!
//! Owns: the wiring of one subcommand from arguments to a result, the
//! logging setup, and the small conveniences every command shares (open
//! the default index, print a warning). Each function returns
//! `Result<u8, Error>`: `Ok(n)` is a contract exit code (the command ran
//! and reports status: query found nothing, doctor found a failure, the
//! action exited non-zero), `Err` means scout could not do what it was
//! asked. `main` turns the `Err` into one stderr line and an exit code.
//! Refuses to know about: clap, `std::process::exit`, the terminal.
//! Exposes: `index`, `open_db`, `query`, `doctor`, `picker`.

pub mod doctor;
pub mod index;
pub mod logging;
pub mod open_db;
pub mod picker;
pub mod query;

pub use doctor::doctor;
pub use index::index;
pub use open_db::open_db;
pub use picker::picker;
pub use query::query;

use rusqlite::Connection;

use crate::{locations, Error};

/// Open the index at its default location.
pub fn open_default_db() -> crate::Result<Connection> {
    let db_path = locations::index_db()?;
    crate::index::open(&db_path).map_err(|err| match err {
        // Keep the path in the message: "open <path>: <cause>".
        Error::Io { context, source } if context == "io" => {
            Error::io(format!("open {}", db_path.display()), source)
        }
        other => other,
    })
}

/// A non-fatal notice for the user. One place owns the prefix.
pub fn warn(message: impl std::fmt::Display) {
    eprintln!("scout: warning: {message}");
}
