//! scout: a terminal project finder and action launcher.
//!
//! The library is the whole tool; `main.rs` only parses arguments and
//! hands them to `commands`. Read the module tree top to bottom to learn
//! the system:
//!
//! - `platform`: every operating-system-specific line, and nothing else.
//! - `locations`: where scout keeps its own files.
//! - `index`: the SQLite file: walking, writing, frecency, recovery.
//! - `search`: ranking candidates for a query.
//! - `recon`: what recon knows about the indexed ground, and the checks.
//! - `actions`: the declarative action model and its executor.
//! - `config`: loading, validating and trusting a config file.
//! - `doctor`: a read-only report of the state scout resolved.
//! - `ui`: the picker.
//! - `commands`: what each subcommand does, as plain functions.
//! - `error`: the one error type and how it becomes an exit code.

pub mod actions;
pub mod commands;
pub mod config;
pub mod doctor;
pub mod error;
pub mod index;
pub mod locations;
pub mod platform;
pub mod recon;
pub mod search;
pub mod ui;

pub use error::Error;

/// Every fallible function in the crate returns this.
pub type Result<T> = std::result::Result<T, Error>;
