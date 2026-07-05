//! Preview content builder — pure data, no ratatui, so the shapes are
//! unit-testable. The pane renders whatever the selected candidate is:
//! a directory listing (dirs first, then files, both sorted), the head
//! of a text file, or a byte-count stub for binaries. Reads are capped
//! hard: this runs on every selection change.

use std::path::Path;

/// Cap on directory entries read and lines shown.
pub const MAX_ENTRIES: usize = 200;
pub const MAX_LINES: usize = 100;
/// One read of at most this many bytes decides a file's preview.
pub const READ_CAP: u64 = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preview {
    Dir {
        /// (name, is_dir), dirs first, each group sorted by name.
        entries: Vec<(String, bool)>,
        /// Entries beyond MAX_ENTRIES exist.
        truncated: bool,
        total: usize,
    },
    TextFile {
        lines: Vec<String>,
        /// File continues beyond what was read.
        truncated: bool,
        size: u64,
    },
    Binary {
        size: u64,
    },
    Unreadable(String),
}

impl Preview {
    /// One-line summary for the pane header, e.g. `dir · 14 entries`.
    pub fn summary(&self) -> String {
        match self {
            Preview::Dir { total, truncated, .. } => {
                let suffix = if *truncated { "+" } else { "" };
                let noun = if *total == 1 && !truncated { "entry" } else { "entries" };
                format!("dir \u{00b7} {total}{suffix} {noun}")
            }
            Preview::TextFile { size, .. } => format!("file \u{00b7} {}", human_size(*size)),
            Preview::Binary { size } => format!("binary \u{00b7} {}", human_size(*size)),
            Preview::Unreadable(_) => "unreadable".into(),
        }
    }
}

pub fn human_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

/// Build the preview for `path`. Never panics; fs problems become
/// `Unreadable` (the candidate may have vanished since indexing —
/// Surgeon §1a treats that as normal terrain, not an error).
pub fn build(path: &Path) -> Preview {
    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(err) => return Preview::Unreadable(err.to_string()),
    };
    if meta.is_dir() {
        build_dir(path)
    } else {
        build_file(path, meta.len())
    }
}

fn build_dir(path: &Path) -> Preview {
    let read_dir = match std::fs::read_dir(path) {
        Ok(rd) => rd,
        Err(err) => return Preview::Unreadable(err.to_string()),
    };
    let mut dirs: Vec<String> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    let mut total = 0usize;
    let mut truncated = false;
    for entry in read_dir.flatten() {
        total += 1;
        if total > MAX_ENTRIES {
            // Stop reading entirely — a 100k-entry dir must not stall a
            // selection change. `total` saturates at the cap.
            truncated = true;
            total = MAX_ENTRIES;
            break;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            dirs.push(name);
        } else {
            files.push(name);
        }
    }
    dirs.sort();
    files.sort();
    let entries: Vec<(String, bool)> = dirs
        .into_iter()
        .map(|n| (n, true))
        .chain(files.into_iter().map(|n| (n, false)))
        .collect();
    Preview::Dir { entries, truncated, total }
}

fn build_file(path: &Path, size: u64) -> Preview {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(err) => return Preview::Unreadable(err.to_string()),
    };
    let mut buf = Vec::with_capacity(READ_CAP as usize);
    if let Err(err) = file.take(READ_CAP).read_to_end(&mut buf) {
        return Preview::Unreadable(err.to_string());
    }
    if buf.contains(&0) {
        return Preview::Binary { size };
    }
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<String> = Vec::with_capacity(MAX_LINES.min(64));
    for line in text.lines().take(MAX_LINES) {
        lines.push(line.to_string());
    }
    let read_all = size <= buf.len() as u64;
    let truncated = !read_all || text.lines().count() > MAX_LINES;
    Preview::TextFile { lines, truncated, size }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "scout-preview-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn dir_preview_sorts_dirs_first() {
        let dir = temp_dir("dir");
        fs::create_dir(dir.join("zeta-dir")).unwrap();
        fs::create_dir(dir.join("alpha-dir")).unwrap();
        fs::write(dir.join("aaa.txt"), b"x").unwrap();
        fs::write(dir.join("bbb.txt"), b"x").unwrap();

        let Preview::Dir { entries, truncated, total } = build(&dir) else {
            panic!("expected dir preview")
        };
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["alpha-dir", "zeta-dir", "aaa.txt", "bbb.txt"]);
        assert!(entries[0].1 && entries[1].1 && !entries[2].1);
        assert!(!truncated);
        assert_eq!(total, 4);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn text_file_preview_heads_lines() {
        let dir = temp_dir("text");
        let file = dir.join("notes.md");
        fs::write(&file, "line one\nline two\nline three\n").unwrap();
        let Preview::TextFile { lines, truncated, .. } = build(&file) else {
            panic!("expected text preview")
        };
        assert_eq!(lines, vec!["line one", "line two", "line three"]);
        assert!(!truncated);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn binary_detected_by_nul() {
        let dir = temp_dir("bin");
        let file = dir.join("blob");
        fs::write(&file, [0x7f, 0x45, 0x4c, 0x46, 0x00, 0x01]).unwrap();
        assert!(matches!(build(&file), Preview::Binary { size: 6 }));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn vanished_path_is_unreadable_not_a_panic() {
        let dir = temp_dir("gone");
        let ghost = dir.join("ghost");
        assert!(matches!(build(&ghost), Preview::Unreadable(_)));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn summaries_and_sizes() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.0 KiB");
        assert_eq!(human_size(3 * 1024 * 1024), "3.0 MiB");
        let p = Preview::Binary { size: 2048 };
        assert_eq!(p.summary(), "binary \u{00b7} 2.0 KiB");
    }
}
