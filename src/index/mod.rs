//! The index: one SQLite file holding every path scout knows and how
//! often it has been used.
//!
//! Owns: the schema and its migrations, the open discipline (private
//! file, owner check, WAL, no symlinks), the streaming walker and the
//! batched writer, the frecency arithmetic and the visit credit, crash
//! recovery, and the pacing that keeps the writer from checkpointing
//! under a live query.
//! Refuses to know about: ranking, config, actions, terminals. It hands
//! out rows and takes credits; what they mean is someone else's job.
//! Exposes: functions over a `rusqlite::Connection`. Every reader filters
//! to the current scan generation, which advances only when a walk
//! completes; a walk that stops early leaves the previous generation
//! serving.

pub mod frecency;
mod open;
pub mod pacing;
pub mod recovery;
pub mod schema;
pub mod walk;
pub mod write;

pub use open::{open, pragma_state};
