//! Everything that knows it is running on a particular operating system.
//!
//! Owns: XDG directory resolution, `O_NOFOLLOW` and the other unix file
//! primitives (private directories and files, owner and mode reads),
//! process spawning shapes, the SHA-256 digest, wall-clock time and its
//! ISO-8601 rendering, and the interrupt flag the signal handlers set.
//! Refuses to know about: what a config, an index, an action or a
//! terminal is. Nothing here imports another scout module except `Error`.
//! Exposes: small portable functions. This is the only directory that may
//! import `std::os::unix::*` or `signal_hook`; a test in `tests/platform.rs`
//! enforces that, so the rest of the crate is portable by construction.

pub mod fs;
pub mod hash;
pub mod process;
pub mod signals;
pub mod time;
pub mod xdg;
