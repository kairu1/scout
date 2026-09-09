//! The checks: each a stable name, a severity, and a pure function over
//! the facts of one path. Pure so the whole table is testable with
//! synthetic modes and uids, without a root shell.

use std::path::Path;

use crate::platform::fs::Facts;

/// Three levels. Two cannot separate "someone else can plant a Makefile
/// here" from "a setuid binary sits in your project"; four invites
/// arguments about the middle. The marker, the confirm prompt and the
/// default exit code key off `High`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Low = 1,
    High = 2,
    Critical = 3,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Low => "low",
            Severity::High => "high",
            Severity::Critical => "critical",
        }
    }

    pub fn parse(name: &str) -> Option<Severity> {
        match name {
            "low" => Some(Severity::Low),
            "high" => Some(Severity::High),
            "critical" => Some(Severity::Critical),
            _ => None,
        }
    }

    pub fn from_i64(value: i64) -> Option<Severity> {
        match value {
            1 => Some(Severity::Low),
            2 => Some(Severity::High),
            3 => Some(Severity::Critical),
            _ => None,
        }
    }
}

/// The closed set of checks. The names are stored in the findings table
/// and accepted by `when = { finding = "..." }`, so they are an interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Check {
    WorldWritableDir,
    WorldWritableFile,
    GroupWritable,
    ForeignOwner,
    Suid,
    SgidFile,
    SgidDir,
    SecretExposed,
    RecentForeignWrite,
    SymlinkEscape,
    AclPresent,
    EntrypointChanged,
}

pub const ALL_CHECKS: [Check; 12] = [
    Check::WorldWritableDir,
    Check::WorldWritableFile,
    Check::GroupWritable,
    Check::ForeignOwner,
    Check::Suid,
    Check::SgidFile,
    Check::SgidDir,
    Check::SecretExposed,
    Check::RecentForeignWrite,
    Check::SymlinkEscape,
    Check::AclPresent,
    Check::EntrypointChanged,
];

impl Check {
    pub fn name(&self) -> &'static str {
        match self {
            Check::WorldWritableDir => "world-writable-dir",
            Check::WorldWritableFile => "world-writable-file",
            Check::GroupWritable => "group-writable",
            Check::ForeignOwner => "foreign-owner",
            Check::Suid => "suid",
            Check::SgidFile => "sgid-file",
            Check::SgidDir => "sgid-dir",
            Check::SecretExposed => "secret-exposed",
            Check::RecentForeignWrite => "recent-foreign-write",
            Check::SymlinkEscape => "symlink-escape",
            Check::AclPresent => "acl-present",
            Check::EntrypointChanged => "entrypoint-changed",
        }
    }

    pub fn parse(name: &str) -> Option<Check> {
        ALL_CHECKS.iter().copied().find(|c| c.name() == name)
    }

    /// One sentence on what the finding means, for the footer and report.
    pub fn meaning(&self) -> &'static str {
        match self {
            Check::WorldWritableDir => "anyone on this machine can add or replace files here",
            Check::WorldWritableFile => "anyone on this machine can rewrite this file",
            Check::GroupWritable => "members of the file's group can write it",
            Check::ForeignOwner => "owned by another user, who controls it",
            Check::Suid => "runs as its owner whoever executes it",
            Check::SgidFile => "runs with its group whoever executes it",
            Check::SgidDir => "files created here take this directory's group",
            Check::SecretExposed => "a credential-shaped file that others can read",
            Check::RecentForeignWrite => "modified in the last day by another user",
            Check::SymlinkEscape => "a link leading outside your home or into a system tree",
            Check::AclPresent => {
                "an ACL is set; effective permissions may be wider than the mode shows"
            }
            Check::EntrypointChanged => "an executable entry point changed since the baseline",
        }
    }

    /// Whether the check costs only the `lstat` the walker already paid
    /// for, and so may run at index time. The symlink check costs a
    /// `readdir` per directory (the index holds canonical paths, so a link
    /// is only visible as a child of an indexed directory), the ACL check
    /// a syscall the walker never made, and the baseline a hash.
    pub fn stat_only(&self) -> bool {
        !matches!(self, Check::AclPresent | Check::EntrypointChanged | Check::SymlinkEscape)
    }
}

/// One finding on one path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub check: Check,
    pub severity: Severity,
    /// Built by scout from the facts; at most `DETAIL_CAP` bytes.
    pub detail: String,
    /// Fingerprint of the fact behind the finding. An accepted exception
    /// holds while the fact is unchanged and lapses when it changes.
    pub fact: String,
}

pub const DETAIL_CAP: usize = 256;

fn finding(check: Check, severity: Severity, detail: String, fact: String) -> Finding {
    let mut detail = detail;
    if detail.len() > DETAIL_CAP {
        let mut cut = DETAIL_CAP;
        while !detail.is_char_boundary(cut) {
            cut -= 1;
        }
        detail.truncate(cut);
    }
    Finding { check, severity, detail, fact }
}

/// Context every evaluation needs.
pub struct Context<'a> {
    pub euid: u32,
    pub now: i64,
    pub home: &'a Path,
}

/// Seconds a modification counts as recent.
pub const RECENT_SECS: i64 = 24 * 3600;

/// Basenames that mark a credential-bearing file. Matched exactly or by
/// the wildcard form. A `*.pub` name never matches.
pub const SECRET_NAMES: &[&str] = &[
    ".env",
    ".env.*",
    "*.pem",
    "*.key",
    "*.p12",
    "*.pfx",
    "*.keystore",
    "*.jks",
    "id_rsa",
    "id_dsa",
    "id_ecdsa",
    "id_ed25519",
    "id_rsa.*",
    "id_dsa.*",
    "id_ecdsa.*",
    "id_ed25519.*",
    ".netrc",
    ".npmrc",
    ".pypirc",
    ".git-credentials",
    "credentials.json",
    "credentials",
    "service-account*.json",
    "kubeconfig",
    ".htpasswd",
    "token",
    ".token",
];

/// True when `name` is on the secret list. Bits decide the finding;
/// this only decides whether the file is the kind whose bits matter.
pub fn is_secret_name(name: &str) -> bool {
    if name.ends_with(".pub") {
        return false;
    }
    SECRET_NAMES.iter().any(|pattern| match pattern.find('*') {
        None => *pattern == name,
        Some(star) => {
            let (prefix, suffix) = (&pattern[..star], &pattern[star + 1..]);
            name.len() > prefix.len() + suffix.len()
                && name.starts_with(prefix)
                && name.ends_with(suffix)
        }
    })
}

/// The line the installer's shell snippet is wrapped in, so recon can
/// say which rc file sources the wrapper without reading anything else
/// in it. `install.sh` and the README carry the same text; a parity
/// test keeps them equal.
pub const WRAPPER_MARKER: &str = "# >>> scout shell integration >>>";

/// Files an action is likely to run: what the integrity baseline hashes.
pub const ENTRY_POINTS: &[&str] = &[
    "Makefile",
    "GNUmakefile",
    "justfile",
    "package.json",
    "Cargo.toml",
    "pyproject.toml",
    "setup.py",
    "go.mod",
    ".envrc",
];

fn mode_fact(f: &Facts) -> String {
    format!("{:o}:{}:{}", f.mode, f.uid, f.gid)
}

/// Evaluate every stat-only check over the facts of one path. `basename`
/// is the final component; `link_target` is `readlink` for a symlink.
pub fn evaluate(
    path: &Path,
    basename: &str,
    f: &Facts,
    link_target: Option<&Path>,
    ctx: &Context<'_>,
) -> Vec<Finding> {
    let mut out = Vec::new();
    let mode = f.mode;
    let fact = mode_fact(f);
    let others_write = mode & 0o002 != 0;
    let group_write = mode & 0o020 != 0;
    let sticky = mode & 0o1000 != 0;

    if f.is_symlink {
        if let Some(target) = link_target {
            let resolved = if target.is_absolute() {
                target.to_path_buf()
            } else {
                path.parent().unwrap_or(path).join(target)
            };
            let system = ["/proc", "/sys", "/dev"].iter().any(|p| resolved.starts_with(p));
            let outside_home = !resolved.starts_with(ctx.home);
            if system {
                out.push(finding(
                    Check::SymlinkEscape,
                    Severity::High,
                    format!("links to {}", resolved.display()),
                    resolved.display().to_string(),
                ));
            } else if outside_home {
                out.push(finding(
                    Check::SymlinkEscape,
                    Severity::Low,
                    format!("links outside your home to {}", resolved.display()),
                    resolved.display().to_string(),
                ));
            }
        }
        // Mode bits on a symlink itself are meaningless on Linux.
        return out;
    }

    if f.is_dir {
        if others_write {
            let (severity, note) = if sticky {
                (Severity::High, " (sticky: others can add but not remove)")
            } else {
                (Severity::Critical, "")
            };
            out.push(finding(
                Check::WorldWritableDir,
                severity,
                format!("mode {:o}{note}", mode & 0o7777),
                fact.clone(),
            ));
        } else if group_write {
            out.push(finding(
                Check::GroupWritable,
                Severity::Low,
                format!("mode {:o}, group {}", mode & 0o777, f.gid),
                fact.clone(),
            ));
        }
        if mode & 0o2000 != 0 {
            out.push(finding(
                Check::SgidDir,
                Severity::Low,
                format!("setgid directory, group {}", f.gid),
                fact.clone(),
            ));
        }
        if basename == ".ssh" && mode & 0o077 != 0 {
            out.push(finding(
                Check::SecretExposed,
                Severity::High,
                format!(".ssh directory is mode {:o}, not 700", mode & 0o777),
                fact.clone(),
            ));
        }
    } else if f.is_file {
        let executable = mode & 0o111 != 0;
        if others_write {
            out.push(finding(
                Check::WorldWritableFile,
                Severity::High,
                format!("mode {:o}", mode & 0o777),
                fact.clone(),
            ));
        } else if group_write {
            out.push(finding(
                Check::GroupWritable,
                Severity::Low,
                format!("mode {:o}, group {}", mode & 0o777, f.gid),
                fact.clone(),
            ));
        }
        if mode & 0o4000 != 0 {
            out.push(finding(
                Check::Suid,
                Severity::Critical,
                format!("setuid, owner uid {}", f.uid),
                fact.clone(),
            ));
        }
        if mode & 0o2000 != 0 {
            out.push(finding(
                Check::SgidFile,
                Severity::High,
                format!("setgid, group {}", f.gid),
                fact.clone(),
            ));
        }
        if is_secret_name(basename) && mode & 0o077 != 0 {
            let severity = if mode & 0o004 != 0 { Severity::Critical } else { Severity::High };
            let who = if mode & 0o004 != 0 { "world" } else { "group" };
            out.push(finding(
                Check::SecretExposed,
                severity,
                format!("{who}-readable credential file, mode {:o}", mode & 0o777),
                fact.clone(),
            ));
        }
        if f.uid != ctx.euid && executable {
            out.push(finding(
                Check::ForeignOwner,
                Severity::High,
                format!("executable owned by uid {}", f.uid),
                fact.clone(),
            ));
        }
    }

    if f.uid != ctx.euid {
        if f.is_dir {
            out.push(finding(
                Check::ForeignOwner,
                Severity::High,
                format!("directory owned by uid {}", f.uid),
                fact.clone(),
            ));
        } else if f.is_file && mode & 0o111 == 0 {
            out.push(finding(
                Check::ForeignOwner,
                Severity::Low,
                format!("owned by uid {}", f.uid),
                fact.clone(),
            ));
        }
        if ctx.now - f.mtime < RECENT_SECS && f.mtime <= ctx.now {
            out.push(finding(
                Check::RecentForeignWrite,
                Severity::High,
                format!("modified {} s ago by uid {}", ctx.now - f.mtime, f.uid),
                format!("{}:{}", f.mtime, f.uid),
            ));
        }
    }
    out
}

/// The ACL check, separated because it costs a syscall the walker did not
/// already pay; it runs only on demand.
pub fn evaluate_acl(has_acl: bool) -> Option<Finding> {
    has_acl.then(|| {
        finding(Check::AclPresent, Severity::Low, "access ACL set".to_string(), "acl".to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(mode: u32, uid: u32, dir: bool) -> Facts {
        Facts {
            uid,
            gid: 100,
            mode,
            is_dir: dir,
            is_file: !dir,
            is_symlink: false,
            mtime: 1_000,
            size: 0,
        }
    }

    fn eval(name: &str, f: &Facts) -> Vec<(Check, Severity)> {
        let ctx = Context { euid: 1000, now: 1_800_000_000, home: Path::new("/home/u") };
        evaluate(Path::new(&format!("/home/u/w/{name}")), name, f, None, &ctx)
            .into_iter()
            .map(|f| (f.check, f.severity))
            .collect()
    }

    #[test]
    fn a_private_tree_owned_by_the_user_has_no_findings() {
        assert!(eval("src", &facts(0o755, 1000, true)).is_empty());
        assert!(eval("main.rs", &facts(0o644, 1000, false)).is_empty());
        assert!(eval("run.sh", &facts(0o755, 1000, false)).is_empty());
    }

    #[test]
    fn world_writable_directory_is_critical_unless_sticky() {
        assert_eq!(
            eval("d", &facts(0o777, 1000, true)),
            vec![(Check::WorldWritableDir, Severity::Critical)]
        );
        assert_eq!(
            eval("d", &facts(0o1777, 1000, true)),
            vec![(Check::WorldWritableDir, Severity::High)]
        );
        assert_eq!(
            eval("f", &facts(0o666, 1000, false)),
            vec![(Check::WorldWritableFile, Severity::High)]
        );
        assert_eq!(
            eval("f", &facts(0o664, 1000, false)),
            vec![(Check::GroupWritable, Severity::Low)]
        );
    }

    #[test]
    fn special_bits_are_found_on_files_and_directories() {
        assert_eq!(
            eval("bin", &facts(0o4755, 1000, false)),
            vec![(Check::Suid, Severity::Critical)]
        );
        assert_eq!(
            eval("bin", &facts(0o2755, 1000, false)),
            vec![(Check::SgidFile, Severity::High)]
        );
        assert_eq!(
            eval("shared", &facts(0o2775, 1000, true)),
            vec![(Check::GroupWritable, Severity::Low), (Check::SgidDir, Severity::Low)]
        );
    }

    #[test]
    fn secrets_are_judged_by_bits_not_content() {
        assert_eq!(
            eval(".env", &facts(0o644, 1000, false)),
            vec![(Check::SecretExposed, Severity::Critical)]
        );
        assert_eq!(
            eval(".env", &facts(0o640, 1000, false)),
            vec![(Check::SecretExposed, Severity::High)]
        );
        assert!(eval(".env", &facts(0o600, 1000, false)).is_empty());
        assert_eq!(
            eval("id_ed25519", &facts(0o644, 1000, false)),
            vec![(Check::SecretExposed, Severity::Critical)]
        );
        assert!(
            eval("id_ed25519.pub", &facts(0o644, 1000, false)).is_empty(),
            "a public key is public"
        );
        assert_eq!(
            eval(".ssh", &facts(0o755, 1000, true)),
            vec![(Check::SecretExposed, Severity::High)]
        );
        assert!(eval(".ssh", &facts(0o700, 1000, true)).is_empty());
    }

    #[test]
    fn secret_name_list_is_lowercase_and_excludes_public_keys() {
        for name in SECRET_NAMES {
            assert_eq!(*name, name.to_lowercase(), "{name}");
        }
        assert!(is_secret_name("server.pem"));
        assert!(is_secret_name(".env.production"));
        assert!(is_secret_name("service-account-prod.json"));
        assert!(!is_secret_name("id_rsa.pub"));
        assert!(!is_secret_name("environment"));
        assert!(!is_secret_name("pem"), "a bare suffix is not a match");
    }

    #[test]
    fn foreign_owner_is_high_for_directories_and_executables_low_for_plain_files() {
        assert_eq!(eval("d", &facts(0o755, 0, true)), vec![(Check::ForeignOwner, Severity::High)]);
        assert_eq!(
            eval("bin", &facts(0o755, 0, false)),
            vec![(Check::ForeignOwner, Severity::High)]
        );
        assert_eq!(
            eval("notes", &facts(0o644, 0, false)),
            vec![(Check::ForeignOwner, Severity::Low)]
        );
    }

    #[test]
    fn a_recent_write_by_someone_else_is_high_and_an_old_one_is_not() {
        let ctx = Context { euid: 1000, now: 1_800_000_000, home: Path::new("/home/u") };
        let mut f = facts(0o644, 0, false);
        f.mtime = ctx.now - 3600;
        let checks: Vec<Check> = evaluate(Path::new("/home/u/w/x"), "x", &f, None, &ctx)
            .into_iter()
            .map(|f| f.check)
            .collect();
        assert_eq!(checks, vec![Check::ForeignOwner, Check::RecentForeignWrite]);
        f.mtime = ctx.now - RECENT_SECS - 1;
        let checks: Vec<Check> = evaluate(Path::new("/home/u/w/x"), "x", &f, None, &ctx)
            .into_iter()
            .map(|f| f.check)
            .collect();
        assert_eq!(checks, vec![Check::ForeignOwner]);
        // The user's own recent edits are not a finding.
        f.uid = 1000;
        f.mtime = ctx.now - 10;
        assert!(evaluate(Path::new("/home/u/w/x"), "x", &f, None, &ctx).is_empty());
    }

    #[test]
    fn symlinks_are_judged_by_where_they_lead() {
        let ctx = Context { euid: 1000, now: 1_800_000_000, home: Path::new("/home/u") };
        let mut f = facts(0o777, 1000, false);
        f.is_file = false;
        f.is_symlink = true;
        let sev = |target: &str| {
            evaluate(Path::new("/home/u/w/link"), "link", &f, Some(Path::new(target)), &ctx)
                .into_iter()
                .map(|f| (f.check, f.severity))
                .collect::<Vec<_>>()
        };
        assert_eq!(sev("/proc/self/root"), vec![(Check::SymlinkEscape, Severity::High)]);
        assert_eq!(sev("/usr/bin/node"), vec![(Check::SymlinkEscape, Severity::Low)]);
        assert!(sev("/home/u/other").is_empty());
        assert!(
            sev("../sibling").is_empty(),
            "relative links resolve against the link's directory"
        );
        // The link's own 777 mode is not a world-writable finding.
    }

    #[test]
    fn detail_is_capped() {
        let long = "x".repeat(1000);
        let f = finding(Check::AclPresent, Severity::Low, long, "acl".into());
        assert_eq!(f.detail.len(), DETAIL_CAP);
    }

    #[test]
    fn every_check_has_a_unique_name_that_round_trips() {
        let mut names: Vec<&str> = ALL_CHECKS.iter().map(|c| c.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ALL_CHECKS.len());
        for check in ALL_CHECKS {
            assert_eq!(Check::parse(check.name()), Some(check));
            assert!(!check.meaning().is_empty());
        }
        assert_eq!(Check::parse("nope"), None);
        assert_eq!(Severity::parse("high"), Some(Severity::High));
        assert_eq!(Severity::from_i64(3), Some(Severity::Critical));
    }
}
