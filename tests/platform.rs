//! The portability guard: every operating-system-specific line lives
//! under `src/platform/`, so the rest of the crate is portable by
//! construction rather than by claim.

use std::path::Path;

fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[test]
fn os_specific_imports_live_only_in_platform() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(files.len() > 20, "walked only {} files; the walk is broken", files.len());

    let forbidden = ["std::os::unix", "std::os::linux", "signal_hook", "extern \"C\"", "libc::"];
    // The FFI declarations are the point of the guard: they may exist only
    // under platform/, and the list above catches them anywhere else.
    let mut offenders = Vec::new();
    for path in &files {
        let rel = path.strip_prefix(&src).unwrap();
        if rel.starts_with("platform") {
            continue;
        }
        let source = std::fs::read_to_string(path).expect("read source");
        for (n, line) in source.lines().enumerate() {
            let trimmed = line.trim_start();
            // Test modules may create symlinks and the like; the guard is
            // about production code. Test modules sit at the end of a file.
            if trimmed.starts_with("#[cfg(test)]") {
                break;
            }
            if trimmed.starts_with("//") {
                continue;
            }
            for needle in forbidden {
                if trimmed.contains(needle) {
                    offenders.push(format!("{}:{} {needle}", rel.display(), n + 1));
                }
            }
        }
    }
    assert!(offenders.is_empty(), "OS-specific code outside src/platform/: {offenders:#?}");
}
