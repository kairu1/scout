//! Applicability: the optional `when` clause that says where an action is
//! offered. Five keys, all ANDed: the selection's kind, a marker file at
//! the selection or its repository root, the file extension, a glob over
//! the canonical path, and a recon finding on the row. An action without
//! a clause is offered everywhere.
//!
//! Evaluation is split in two so the picker never stats per frame: an
//! `Applicability` is computed once per selected path (a handful of
//! stats), and every action's clause is then checked against it in
//! memory.

use std::collections::HashSet;
use std::path::Path;

use globset::{GlobBuilder, GlobMatcher};

use super::template::repo_root;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A directory holding a `.git` entry.
    Repo,
    Dir,
    File,
}

impl Kind {
    pub fn parse(name: &str) -> Option<Kind> {
        match name {
            "repo" => Some(Kind::Repo),
            "dir" => Some(Kind::Dir),
            "file" => Some(Kind::File),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Kind::Repo => "repo",
            Kind::Dir => "dir",
            Kind::File => "file",
        }
    }
}

/// A parsed `when` clause. Every field is optional; at least one is set.
#[derive(Debug, Clone)]
pub struct When {
    pub kind: Option<Kind>,
    /// Any one of these present at the selection (its parent for a file)
    /// or at the repository root satisfies the clause.
    pub marker: Vec<String>,
    /// Extensions without the dot; files only. False on a directory.
    pub ext: Vec<String>,
    /// The pattern as written, for the trust projection and the footer.
    pub glob: Option<String>,
    glob_matcher: Option<GlobMatcher>,
    /// A recon check name; satisfied by an unaccepted finding on the row.
    pub finding: Option<String>,
}

impl When {
    /// Build a clause from validated parts. `home` expands a leading `~/`
    /// in the glob; the raw pattern is kept for hashing so the projection
    /// does not depend on the machine.
    pub fn new(
        kind: Option<Kind>,
        marker: Vec<String>,
        ext: Vec<String>,
        glob: Option<String>,
        finding: Option<String>,
        home: &str,
    ) -> Result<When, String> {
        let glob_matcher = match &glob {
            Some(pattern) => {
                let expanded = match pattern.strip_prefix("~/") {
                    Some(rest) => format!("{}/{rest}", home.trim_end_matches('/')),
                    None => pattern.clone(),
                };
                let glob = GlobBuilder::new(&expanded)
                    .literal_separator(true)
                    .build()
                    .map_err(|e| format!("glob `{pattern}`: {e}"))?;
                Some(glob.compile_matcher())
            }
            None => None,
        };
        Ok(When { kind, marker, ext, glob, glob_matcher, finding })
    }

    /// True when the selection satisfies every key present.
    pub fn applies(&self, a: &Applicability) -> bool {
        if let Some(kind) = self.kind {
            if a.kind != Some(kind) {
                return false;
            }
        }
        if !self.marker.is_empty() && !self.marker.iter().any(|m| a.markers_present.contains(m)) {
            return false;
        }
        if !self.ext.is_empty() {
            match &a.ext {
                Some(ext) if self.ext.iter().any(|e| e.eq_ignore_ascii_case(ext)) => {}
                _ => return false,
            }
        }
        if let Some(matcher) = &self.glob_matcher {
            if !matcher.is_match(&a.path) {
                return false;
            }
        }
        if let Some(finding) = &self.finding {
            if !a.findings.contains(finding) {
                return false;
            }
        }
        true
    }

    /// The clause as a phrase for the footer: why an action is not offered.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(kind) = self.kind {
            parts.push(match kind {
                Kind::Repo => "needs a git repository".to_string(),
                Kind::Dir => "needs a directory".to_string(),
                Kind::File => "needs a file".to_string(),
            });
        }
        if !self.marker.is_empty() {
            parts.push(format!("needs {} here or at the repo root", self.marker.join(" or ")));
        }
        if !self.ext.is_empty() {
            parts.push(format!("needs a .{} file", self.ext.join(" or .")));
        }
        if let Some(glob) = &self.glob {
            parts.push(format!("needs a path matching {glob}"));
        }
        if let Some(finding) = &self.finding {
            parts.push(format!("needs a {finding} finding"));
        }
        parts.join("; ")
    }
}

/// What is true of one selected path, computed once per selection.
#[derive(Debug, Clone, Default)]
pub struct Applicability {
    pub path: String,
    /// `None` when the path could not be stat'ed (it vanished).
    pub kind: Option<Kind>,
    pub markers_present: HashSet<String>,
    pub ext: Option<String>,
    /// Unaccepted recon finding names on this row. Empty until recon
    /// stores some.
    pub findings: HashSet<String>,
}

impl Applicability {
    /// Stat the selection once, and probe each of `markers` at the
    /// directory the selection is (or lives in) and at its repository
    /// root. `markers` is the union of every action's `marker` list, so
    /// the cost is bounded by the config, not by the number of actions.
    pub fn for_path(path: &Path, markers: &[String], findings: HashSet<String>) -> Applicability {
        let meta = std::fs::metadata(path).ok();
        let kind = meta.as_ref().map(|m| {
            if m.is_dir() {
                if path.join(".git").exists() {
                    Kind::Repo
                } else {
                    Kind::Dir
                }
            } else {
                Kind::File
            }
        });
        let ext = match kind {
            Some(Kind::File) => {
                Some(path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default())
            }
            _ => None,
        };
        let here: Option<&Path> = match kind {
            Some(Kind::File) => path.parent(),
            Some(_) => Some(path),
            None => None,
        };
        let mut markers_present = HashSet::new();
        if !markers.is_empty() {
            let mut dirs: Vec<std::path::PathBuf> = Vec::new();
            if let Some(here) = here {
                dirs.push(here.to_path_buf());
            }
            if let Some(root) = repo_root(path) {
                if !dirs.contains(&root) {
                    dirs.push(root);
                }
            }
            for dir in dirs {
                for marker in markers {
                    if dir.join(marker).exists() {
                        markers_present.insert(marker.clone());
                    }
                }
            }
        }
        Applicability { path: path.display().to_string(), kind, markers_present, ext, findings }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn when(kind: Option<Kind>, marker: &[&str], ext: &[&str], glob: Option<&str>) -> When {
        When::new(
            kind,
            marker.iter().map(|s| s.to_string()).collect(),
            ext.iter().map(|s| s.to_string()).collect(),
            glob.map(str::to_string),
            None,
            "/home/u",
        )
        .unwrap()
    }

    fn appl(kind: Option<Kind>, markers: &[&str], ext: Option<&str>, path: &str) -> Applicability {
        Applicability {
            path: path.to_string(),
            kind,
            markers_present: markers.iter().map(|s| s.to_string()).collect(),
            ext: ext.map(str::to_string),
            findings: HashSet::new(),
        }
    }

    #[test]
    fn keys_are_anded_and_marker_entries_are_any_of() {
        let repo_with_cargo = appl(Some(Kind::Repo), &["Cargo.toml"], None, "/home/u/w/api");
        assert!(when(Some(Kind::Repo), &["Cargo.toml"], &[], None).applies(&repo_with_cargo));
        assert!(when(None, &["package.json", "Cargo.toml"], &[], None).applies(&repo_with_cargo));
        assert!(!when(Some(Kind::Dir), &["Cargo.toml"], &[], None).applies(&repo_with_cargo));
        assert!(!when(None, &["package.json"], &[], None).applies(&repo_with_cargo));
    }

    #[test]
    fn ext_is_false_on_a_directory_and_case_insensitive_on_a_file() {
        let dir = appl(Some(Kind::Dir), &[], None, "/home/u/w");
        assert!(!when(None, &[], &["rs"], None).applies(&dir));
        let file = appl(Some(Kind::File), &[], Some("RS"), "/home/u/w/main.RS");
        assert!(when(None, &[], &["rs"], None).applies(&file));
        let no_ext = appl(Some(Kind::File), &[], Some(""), "/home/u/w/Makefile");
        assert!(!when(None, &[], &["rs"], None).applies(&no_ext));
    }

    #[test]
    fn glob_expands_home_and_does_not_cross_separators_with_a_single_star() {
        let inside = appl(Some(Kind::Dir), &[], None, "/home/u/work/api");
        let deeper = appl(Some(Kind::Dir), &[], None, "/home/u/work/billing/api");
        let outside = appl(Some(Kind::Dir), &[], None, "/srv/work/api");
        assert!(when(None, &[], &[], Some("~/work/*")).applies(&inside));
        assert!(!when(None, &[], &[], Some("~/work/*")).applies(&deeper));
        assert!(when(None, &[], &[], Some("~/work/**")).applies(&deeper));
        assert!(!when(None, &[], &[], Some("~/work/**")).applies(&outside));
    }

    #[test]
    fn finding_requires_the_named_finding_on_the_row() {
        let clause = When::new(None, vec![], vec![], None, Some("suid".into()), "/h").unwrap();
        let mut a = appl(Some(Kind::File), &[], Some(""), "/x");
        assert!(!clause.applies(&a));
        a.findings.insert("suid".into());
        assert!(clause.applies(&a));
    }

    #[test]
    fn a_vanished_selection_satisfies_no_kind_clause_but_an_empty_one_still_applies() {
        let gone = appl(None, &[], None, "/gone");
        assert!(!when(Some(Kind::Dir), &[], &[], None).applies(&gone));
        assert!(!when(Some(Kind::File), &[], &[], None).applies(&gone));
    }

    #[test]
    fn describe_names_each_requirement() {
        let text = when(Some(Kind::Repo), &["Cargo.toml", "package.json"], &["rs"], Some("~/w/**"))
            .describe();
        assert!(text.contains("git repository"), "{text}");
        assert!(text.contains("Cargo.toml or package.json"), "{text}");
        assert!(text.contains(".rs"), "{text}");
        assert!(text.contains("~/w/**"), "{text}");
    }

    #[test]
    fn for_path_stats_kind_markers_and_ext() {
        let dir = std::env::temp_dir().join(format!("scout-when-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = dir.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("Cargo.toml"), "").unwrap();
        std::fs::write(repo.join("src/main.rs"), "").unwrap();
        let markers = vec!["Cargo.toml".to_string(), "package.json".to_string()];

        let at_root = Applicability::for_path(&repo, &markers, HashSet::new());
        assert_eq!(at_root.kind, Some(Kind::Repo));
        assert!(at_root.markers_present.contains("Cargo.toml"));
        assert!(!at_root.markers_present.contains("package.json"));
        assert_eq!(at_root.ext, None);

        // A file three levels in still sees the repo root's marker.
        let file = Applicability::for_path(&repo.join("src/main.rs"), &markers, HashSet::new());
        assert_eq!(file.kind, Some(Kind::File));
        assert_eq!(file.ext.as_deref(), Some("rs"));
        assert!(file.markers_present.contains("Cargo.toml"));

        let plain = Applicability::for_path(&repo.join("src"), &markers, HashSet::new());
        assert_eq!(plain.kind, Some(Kind::Dir));
        assert!(plain.markers_present.contains("Cargo.toml"), "found at the repo root");

        let gone = Applicability::for_path(&dir.join("missing"), &markers, HashSet::new());
        assert_eq!(gone.kind, None);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
