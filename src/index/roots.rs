//! The trees the index holds: one row per root, with the generation of
//! its last completed walk and the flags that walk used.
//!
//! Owns: listing, resolving a path to the root that holds it, creating a
//! root, storing walk flags, and forgetting a root. Refuses to know
//! about: walking, rows, ranking. Exposes: `Root`, `WalkFlags`, `list`,
//! `resolve`, `ensure`, `forget`.
//!
//! Roots never nest. A path inside an existing root means that root; a
//! path that contains an existing root is refused, because two roots
//! walking the same rows would fight over their generations.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::{Error, Result};

/// One indexed tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    pub id: i64,
    pub path: PathBuf,
    /// Number of the last completed walk of this root; 0 before one.
    pub current_generation: i64,
    pub hidden: bool,
    pub follow: bool,
    /// The cheap recon checks run during the walk.
    pub recon: bool,
    pub last_walk_started_at: Option<i64>,
    pub last_walk_completed_at: Option<i64>,
}

/// How a walk is asked to run. `None` means "as the root was walked
/// last time" (or the default for a new root: not hidden, no symlinks,
/// recon on).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WalkFlags {
    pub hidden: Option<bool>,
    pub follow: Option<bool>,
    pub recon: Option<bool>,
}

/// What a path resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// The root that holds the path (equal to it or an ancestor).
    Existing(Root),
    /// No root holds it and it holds no root: a root to be.
    New(PathBuf),
}

const COLUMNS: &str = "id, path, current_generation, hidden, follow, recon, \
                       last_walk_started_at, last_walk_completed_at";

fn row_to_root(r: &rusqlite::Row<'_>) -> rusqlite::Result<Root> {
    Ok(Root {
        id: r.get(0)?,
        path: PathBuf::from(r.get::<_, String>(1)?),
        current_generation: r.get(2)?,
        hidden: r.get::<_, i64>(3)? != 0,
        follow: r.get::<_, i64>(4)? != 0,
        recon: r.get::<_, i64>(5)? != 0,
        last_walk_started_at: r.get(6)?,
        last_walk_completed_at: r.get(7)?,
    })
}

/// Every root, in the order they were added.
pub fn list(conn: &Connection) -> Result<Vec<Root>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM roots ORDER BY id"))?;
    let roots = stmt.query_map([], row_to_root)?.collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(roots)
}

/// The root at exactly `path`, if any.
pub fn by_path(conn: &Connection, path: &Path) -> Result<Option<Root>> {
    let text = path.display().to_string();
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM roots WHERE path = :path"))?;
    let mut rows = stmt.query(rusqlite::named_params! { ":path": text })?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_root(row)?)),
        None => Ok(None),
    }
}

/// Which root `canonical` belongs to. Refuses a path that would nest an
/// existing root inside it.
pub fn resolve(conn: &Connection, canonical: &Path) -> Result<Resolved> {
    for root in list(conn)? {
        if canonical == root.path || canonical.starts_with(&root.path) {
            return Ok(Resolved::Existing(root));
        }
        if root.path.starts_with(canonical) {
            return Err(Error::NestedRoot {
                given: canonical.to_path_buf(),
                existing: root.path.clone(),
            });
        }
    }
    Ok(Resolved::New(canonical.to_path_buf()))
}

/// The root for `canonical`, created if new, with `flags` applied over
/// what is stored (or over the defaults for a new root). Returns the
/// root as it will be walked.
pub fn ensure(conn: &Connection, canonical: &Path, flags: WalkFlags) -> Result<Root> {
    let root = match resolve(conn, canonical)? {
        Resolved::Existing(root) => root,
        Resolved::New(path) => {
            conn.execute(
                "INSERT INTO roots (path) VALUES (:path)",
                rusqlite::named_params! { ":path": path.display().to_string() },
            )?;
            by_path(conn, &path)?.expect("root row just inserted")
        }
    };
    let merged = Root {
        hidden: flags.hidden.unwrap_or(root.hidden),
        follow: flags.follow.unwrap_or(root.follow),
        recon: flags.recon.unwrap_or(root.recon),
        ..root
    };
    conn.execute(
        "UPDATE roots SET hidden = :hidden, follow = :follow, recon = :recon WHERE id = :id",
        rusqlite::named_params! {
            ":hidden": merged.hidden as i64,
            ":follow": merged.follow as i64,
            ":recon": merged.recon as i64,
            ":id": merged.id,
        },
    )?;
    Ok(merged)
}

/// Forget a root: tombstone its rows now (a later walk revives them with
/// their history) and delete the root. Returns the rows tombstoned, or
/// `None` when no root sits at exactly that path.
pub fn forget(conn: &Connection, canonical: &Path, now: i64) -> Result<Option<u64>> {
    let Some(root) = by_path(conn, canonical)? else { return Ok(None) };
    let tx = conn.unchecked_transaction()?;
    let tombstoned = tx.execute(
        "UPDATE paths SET tombstoned_at = :now WHERE root_id = :id AND tombstoned_at IS NULL",
        rusqlite::named_params! { ":now": now, ":id": root.id },
    )?;
    // What recon knew about the tree goes with it: a forgotten tree's
    // findings must not keep every later report failing.
    tx.execute(
        "DELETE FROM findings WHERE path_id IN (SELECT rowid FROM paths WHERE root_id = :id)",
        rusqlite::named_params! { ":id": root.id },
    )?;
    tx.execute(
        "DELETE FROM baseline WHERE path_id IN (SELECT rowid FROM paths WHERE root_id = :id)",
        rusqlite::named_params! { ":id": root.id },
    )?;
    tx.execute(
        "UPDATE paths SET worst_finding = 0 WHERE root_id = :id",
        rusqlite::named_params! { ":id": root.id },
    )?;
    tx.execute("DELETE FROM roots WHERE id = :id", rusqlite::named_params! { ":id": root.id })?;
    tx.commit()?;
    Ok(Some(tombstoned as u64))
}
