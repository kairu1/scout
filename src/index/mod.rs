//! Index/DB sector (2nd Rifles). Binds: ADR-001 (schema, pacing, visit
//! credit), ADR-002 Phase 2 admission list, ADR-003 §1/§4 (parameter-bound
//! SQL, DB permissions, refusal boundaries).

pub mod insert;
pub mod pragma;
pub mod schema;
pub mod signals;
pub mod walk;

use std::fmt;

#[derive(Debug)]
pub enum IndexError {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    /// Refused at a boundary per ADR-003 (symlinked DB, denylisted path,
    /// hazardous bytes, wrong owner). The string names what was refused.
    Refused(String),
    /// Integrity failure on the recovery path.
    Corrupt(String),
}

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IndexError::Sqlite(e) => write!(f, "sqlite: {e}"),
            IndexError::Io(e) => write!(f, "io: {e}"),
            IndexError::Refused(what) => write!(f, "refused: {what}"),
            IndexError::Corrupt(what) => write!(f, "corrupt: {what}"),
        }
    }
}

impl std::error::Error for IndexError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            IndexError::Sqlite(e) => Some(e),
            IndexError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for IndexError {
    fn from(e: rusqlite::Error) -> Self {
        IndexError::Sqlite(e)
    }
}

impl From<std::io::Error> for IndexError {
    fn from(e: std::io::Error) -> Self {
        IndexError::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, IndexError>;

/// Unix seconds now. SystemTime per ADR-002 (`chrono` refused).
pub(crate) fn unix_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
