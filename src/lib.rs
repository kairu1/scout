//! scout — a terminal project finder and action launcher.

/// `O_NOFOLLOW`, hand-pinned to avoid a `libc` dependency (ADR-002).
///
/// Defined ONCE. It was copied verbatim into two modules, each with its
/// own `cfg` ladder: both were correct, and nothing kept them in step —
/// a per-arch value that silently no-ops on the arch you did not test is
/// exactly the failure this constant exists to prevent.
#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "x86")))]
pub(crate) const O_NOFOLLOW: i32 = 0o400000;
#[cfg(all(target_os = "linux", not(any(target_arch = "x86_64", target_arch = "x86"))))]
pub(crate) const O_NOFOLLOW: i32 = 0o100000;
#[cfg(target_os = "macos")]
pub(crate) const O_NOFOLLOW: i32 = 0x0100;

pub mod actions;
pub mod config;
pub mod doctor;
pub mod index;
pub mod ipc;
pub mod search;
pub mod ui;
