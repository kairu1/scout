//! Extended-attribute presence, via two libc symbols declared by hand.
//! Rust's standard library has no xattr access, and the only thing recon
//! needs to know this round is whether a POSIX ACL is set at all: the ACL
//! lives in the `system.posix_acl_access` attribute, so listing attribute
//! names answers the question without parsing the ACL body.
//!
//! `llistxattr` and `lgetxattr` are exported by glibc and by musl with the
//! same signature on every Linux architecture; declaring them costs no
//! crate. Non-Linux builds report "no ACL" and say so in the docs.

use std::io;
use std::path::Path;

#[cfg(target_os = "linux")]
mod sys {
    use std::ffi::CString;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    extern "C" {
        fn llistxattr(
            path: *const std::ffi::c_char,
            list: *mut std::ffi::c_char,
            size: usize,
        ) -> isize;
    }

    /// Names of the extended attributes on `path` itself (a symlink is
    /// not followed). Empty when the filesystem has none or does not
    /// support them.
    pub fn attribute_names(path: &Path) -> io::Result<Vec<String>> {
        let c_path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in path"))?;
        // 64 KiB is the kernel's ceiling for a single attribute list.
        let mut buf = vec![0u8; 64 * 1024];
        // SAFETY: `c_path` is a valid NUL-terminated string that outlives
        // the call; `buf` is a writable buffer of the length passed.
        let len = unsafe { llistxattr(c_path.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
        if len < 0 {
            let err = io::Error::last_os_error();
            return match err.raw_os_error() {
                // ENOTSUP / EOPNOTSUPP: the filesystem has no xattrs;
                // ENODATA: none set. Both mean "no ACL".
                Some(95) | Some(61) => Ok(Vec::new()),
                _ => Err(err),
            };
        }
        buf.truncate(len as usize);
        Ok(buf
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect())
    }
}

/// True when `path` carries a POSIX access or default ACL.
pub fn has_posix_acl(path: &Path) -> io::Result<bool> {
    #[cfg(target_os = "linux")]
    {
        let names = sys::attribute_names(path)?;
        Ok(names.iter().any(|n| n == "system.posix_acl_access" || n == "system.posix_acl_default"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file scout just created has no ACL. The positive case needs
    /// `setfacl`; the ignored test in `tests/recon.rs` sets one and CI
    /// runs it with the `acl` package installed.
    #[test]
    fn a_fresh_file_has_no_acl() {
        let dir = std::env::temp_dir().join(format!("scout-xattr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("plain");
        std::fs::write(&file, b"x").unwrap();
        assert!(!has_posix_acl(&file).unwrap());
        assert!(!has_posix_acl(&dir).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_path_is_an_error_not_a_false() {
        assert!(has_posix_acl(Path::new("/nonexistent/scout/path")).is_err());
    }
}
