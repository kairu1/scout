//! Recon: what scout can tell you about the ground it indexes. Who else
//! can edit your projects, what is exposed by its permissions, what
//! changed since you last looked.
//!
//! Owns: the closed set of checks and their severities, the pure
//! evaluation of a check over the facts of one path, the findings and
//! exceptions tables and the `worst_finding` column that summarises them,
//! the integrity baseline, and the report renderings.
//! Refuses to: repair anything, read file contents (the secret-name check
//! is about permission bits, never about what is inside), or stat on the
//! picker's frame path. Recon writes only its own tables; the paths it
//! inspects are never touched.
//! Exposes: `Severity`, `Check`, `evaluate`, the store functions, and
//! `Report`.

pub mod checks;
pub mod report;
pub mod run;
pub mod store;

pub use checks::{evaluate, Check, Finding, Severity, ALL_CHECKS};
pub use report::Report;
