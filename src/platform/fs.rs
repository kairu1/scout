//! Unix file primitives: open without following a symlink, create
//! private (0700 / 0600) directories and files, read owner and mode.
//! Policy about *which* files must be private lives with the file's
//! owner module; the mechanism lives here.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

/// `O_NOFOLLOW`, pinned per architecture rather than taken from a crate.
/// Defined once: it was copied into two modules before, and a per-arch
/// value that silently no-ops on the arch you did not test is exactly
/// the failure this constant exists to prevent.
#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "x86")))]
pub const O_NOFOLLOW: i32 = 0o400000;
#[cfg(all(target_os = "linux", not(any(target_arch = "x86_64", target_arch = "x86"))))]
pub const O_NOFOLLOW: i32 = 0o100000;
#[cfg(target_os = "macos")]
pub const O_NOFOLLOW: i32 = 0x0100;

/// Open `path` for reading without following a final-component symlink.
/// `Ok(Some(file))` is a regular file; `Ok(None)` means absent, a symlink,
/// or not a regular file (callers treat all three as "fall through");
/// `Err` is a real I/O failure worth surfacing.
pub fn open_nofollow(path: &Path) -> io::Result<Option<File>> {
    match OpenOptions::new().read(true).custom_flags(O_NOFOLLOW).open(path) {
        Ok(file) => {
            if file.metadata()?.is_file() {
                Ok(Some(file))
            } else {
                Ok(None)
            }
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) if is_symlink(path) => Ok(None),
        Err(err) => Err(err),
    }
}

/// True when the final component of `path` is a symlink (dangling or not).
pub fn is_symlink(path: &Path) -> bool {
    path.symlink_metadata().map(|m| m.file_type().is_symlink()).unwrap_or(false)
}

/// Create `dir` and any missing parents with mode 0700. A pre-existing
/// directory is left as it is: its mode is the user's call.
pub fn create_private_dir(dir: &Path) -> io::Result<()> {
    if dir.exists() {
        return Ok(());
    }
    fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)
}

/// Open `path` read-write, creating it 0600 if absent, refusing to follow
/// a symlink at the final component. Returns whether the file existed.
pub fn open_or_create_private(path: &Path) -> io::Result<(File, bool)> {
    let existed = path.symlink_metadata().is_ok();
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW)
        .open(path)?;
    if !existed {
        // umask-agnostic: `mode(0o600)` is masked; this is not.
        set_private(path)?;
    }
    Ok((file, existed))
}

/// Open `path` for appending without following a symlink, creating it
/// 0600 when absent. The print sink: a line for the shell wrapper goes
/// into the file it named, never into wherever a link points.
pub fn open_append_nofollow(path: &Path) -> io::Result<File> {
    OpenOptions::new().append(true).create(true).mode(0o600).custom_flags(O_NOFOLLOW).open(path)
}

/// Create `path` 0600 if it does not exist yet; leave it alone otherwise.
pub fn ensure_private_file(path: &Path) -> io::Result<()> {
    if path.symlink_metadata().is_err() {
        OpenOptions::new().write(true).create(true).truncate(false).mode(0o600).open(path)?;
        set_private(path)?;
    }
    Ok(())
}

/// Write `body` to `path` so that the file is 0600 at every instant of
/// its existence: created with the mode, then truncated and written.
pub fn write_private(path: &Path, body: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut file =
        OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    file.write_all(body)?;
    set_private(path)
}

/// chmod 0600.
pub fn set_private(path: &Path) -> io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

/// Owner uid of `path` (following symlinks).
pub fn owner_uid(path: &Path) -> io::Result<u32> {
    Ok(fs::metadata(path)?.uid())
}

/// Permission bits of `path` (following symlinks), masked to 0o7777.
pub fn mode_bits(path: &Path) -> io::Result<u32> {
    Ok(fs::metadata(path)?.mode() & 0o7777)
}

extern "C" {
    fn geteuid() -> u32;
}

/// The effective uid of this process. One libc symbol, present on every
/// unix; it replaces a probe file that had to be created and deleted to
/// learn the same number.
pub fn euid() -> u32 {
    // SAFETY: geteuid takes no arguments and cannot fail.
    unsafe { geteuid() }
}

/// Everything recon wants to know about one path, from a single `lstat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Facts {
    pub uid: u32,
    pub gid: u32,
    /// Permission bits including the setuid, setgid and sticky bits.
    pub mode: u32,
    pub is_dir: bool,
    pub is_file: bool,
    pub is_symlink: bool,
    /// Modification time, unix seconds.
    pub mtime: i64,
    pub size: u64,
}

/// `lstat` the path: a symlink is described, not followed.
pub fn facts(path: &Path) -> io::Result<Facts> {
    let m = fs::symlink_metadata(path)?;
    Ok(Facts {
        uid: m.uid(),
        gid: m.gid(),
        mode: m.mode() & 0o7777,
        is_dir: m.is_dir(),
        is_file: m.is_file(),
        is_symlink: m.file_type().is_symlink(),
        mtime: m.mtime(),
        size: m.len(),
    })
}
