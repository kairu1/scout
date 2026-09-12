//! The release installer, driven against a fake release served from a
//! `file://` base: the same command installs, updates in place, rewrites
//! a stale rc block, and refuses a bad checksum, a mis-versioned or
//! incomplete tarball, and a missing release. Then `scout recon`, run
//! from the installed layout, sees the installed wrapper. Needs `curl`
//! (with the file protocol), `tar` and `sha256sum` on PATH, the
//! installer's own requirements; skipped otherwise.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn have(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

fn curl_reads_files() -> bool {
    Command::new("curl")
        .args(["-fsSL", "file:///dev/null"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// A private HOME, PREFIX, rc file and release directory, removed on
/// drop so a failed assertion leaves no tarball behind.
struct Sandbox {
    dir: PathBuf,
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

impl Sandbox {
    fn new() -> Sandbox {
        let dir = std::env::temp_dir().join(format!("scout-installer-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for sub in ["home", "prefix", "dist", "tree/alpha"] {
            fs::create_dir_all(dir.join(sub)).unwrap();
        }
        Sandbox { dir }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.join(rel)
    }

    /// The four-member tarball the release workflow produces, at
    /// `dist/v<version>/`, with `payload` as the `scout` member. A
    /// `skip` member is left out to fake a broken release.
    fn release(&self, version: &str, payload: Payload, skip: Option<&str>) -> String {
        let name = format!("scout-{version}-{}", target());
        let release = self.path("dist").join(format!("v{version}"));
        let stage = release.join(&name);
        fs::create_dir_all(&stage).unwrap();
        match payload {
            Payload::RealBinary => {
                fs::copy(env!("CARGO_BIN_EXE_scout"), stage.join("scout")).unwrap();
            }
            Payload::Reports(v) => {
                fs::write(stage.join("scout"), format!("#!/bin/sh\necho scout {v}\n")).unwrap();
                fs::set_permissions(stage.join("scout"), fs::Permissions::from_mode(0o755))
                    .unwrap();
            }
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        for (member, src) in [
            ("scout.bash", root.join("shell/scout.bash")),
            ("config.toml", root.join("examples/config.toml")),
            ("README.md", root.join("README.md")),
        ] {
            if skip != Some(member) {
                fs::copy(src, stage.join(member)).unwrap();
            }
        }
        assert!(Command::new("tar")
            .current_dir(&release)
            .args(["czf", &format!("{name}.tar.gz"), &name])
            .status()
            .unwrap()
            .success());
        let sum = Command::new("sha256sum")
            .current_dir(&release)
            .arg(format!("{name}.tar.gz"))
            .output()
            .unwrap();
        fs::write(release.join(format!("{name}.tar.gz.sha256")), sum.stdout).unwrap();
        fs::remove_dir_all(&stage).unwrap();
        name
    }

    fn run(&self, version: &str) -> Output {
        Command::new("sh")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("install-release.sh"))
            .arg(version)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", self.path("home"))
            .env("PREFIX", self.path("prefix"))
            .env("SCOUT_RC", self.path("home/.bashrc"))
            .env("SCOUT_RELEASE_BASE", format!("file://{}", self.path("dist").display()))
            .env("XDG_CONFIG_HOME", self.path("home/.config"))
            .output()
            .unwrap()
    }

    fn rc(&self) -> String {
        fs::read_to_string(self.path("home/.bashrc")).unwrap()
    }

    fn bin(&self) -> Vec<u8> {
        fs::read(self.path("prefix/bin/scout")).unwrap()
    }
}

enum Payload {
    RealBinary,
    /// A shell script that answers `--version` with this version.
    Reports(&'static str),
}

fn target() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "aarch64-unknown-linux-musl"
    } else {
        "x86_64-unknown-linux-musl"
    }
}

fn text(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn installs_updates_in_place_and_refuses_what_it_cannot_trust() {
    if !(have("curl") && have("tar") && have("sha256sum") && curl_reads_files()) {
        eprintln!("curl (with file://), tar or sha256sum missing; skipping");
        return;
    }
    let sb = Sandbox::new();
    let marker = scout::recon::checks::WRAPPER_MARKER;
    let end = scout::recon::checks::WRAPPER_MARKER_END;
    let wrapper = sb.path("prefix/share/scout-dist/scout.bash");
    let source_line = format!("source \"{}\"", wrapper.display());

    // 1. First install of a payload that reports 9.9.9: binary, snippet,
    //    reference config at their modes; the rc block once, quoted;
    //    nothing in the config chain; scout's own data dir untouched.
    let name = sb.release("9.9.9", Payload::Reports("9.9.9"), None);
    let out = sb.run("9.9.9");
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(mode(&sb.path("prefix/bin/scout")), 0o755);
    assert_eq!(mode(&wrapper), 0o644);
    assert_eq!(mode(&sb.path("prefix/share/scout-dist/config.toml")), 0o644);
    assert!(!sb.path("prefix/share/scout").exists(), "the data dir is not the installer's");
    assert!(!sb.path("home/.config/scout/config.toml").exists(), "never writes a config");
    let rc = sb.rc();
    assert_eq!(rc.matches(marker).count(), 1, "{rc}");
    assert_eq!(rc.matches(end).count(), 1, "{rc}");
    assert!(rc.contains(&source_line), "{rc}");
    let t = text(&out);
    assert!(t.contains("installed: scout 9.9.9"), "{t}");
    assert!(t.contains("no config at"), "{t}");
    assert!(!sb.path("dist/v9.9.9").join(&name).exists());

    // 2. The same version again: nothing to say about the rc.
    let out = sb.run("9.9.9");
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("already in"), "{}", text(&out));
    assert_eq!(sb.rc().matches(marker).count(), 1);

    // 3. Update to the real binary, whose version is this crate's: the
    //    `updated:` line, and the binary runs from where it landed.
    let version = env!("CARGO_PKG_VERSION");
    sb.release(version, Payload::RealBinary, None);
    let out = sb.run(version);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains(&format!("updated: scout 9.9.9 -> scout {version}")),
        "{}",
        text(&out)
    );
    let real = sb.bin();

    // 4. An rc block that sources an older location is rewritten as one
    //    unit: still one block, now pointing here, everything else kept.
    fs::write(
        sb.path("home/.bashrc"),
        format!("export X=1\n\n{marker}\nsource /opt/old/shell/scout.bash\n{end}\nexport Y=2\n"),
    )
    .unwrap();
    let out = sb.run(version);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("updated in"), "{}", text(&out));
    let rc = sb.rc();
    assert_eq!(rc.matches(marker).count(), 1, "{rc}");
    assert!(rc.contains(&source_line) && !rc.contains("/opt/old/"), "{rc}");
    assert!(rc.contains("export X=1") && rc.contains("export Y=2"), "{rc}");

    // 5. A tarball that does not match its checksum installs nothing.
    let real_name = format!("scout-{version}-{}", target());
    let sums =
        sb.path("dist").join(format!("v{version}")).join(format!("{real_name}.tar.gz.sha256"));
    let good = fs::read_to_string(&sums).unwrap();
    let flipped = if good.starts_with('0') { "1" } else { "0" };
    fs::write(&sums, format!("{flipped}{}", &good[1..])).unwrap();
    let out = sb.run(version);
    assert!(!out.status.success());
    assert!(text(&out).contains("checksum mismatch"), "{}", text(&out));
    assert_eq!(sb.bin(), real, "binary untouched");
    fs::write(&sums, good).unwrap();

    // 6. A release that does not exist fails before touching anything.
    let out = sb.run("0.0.0");
    assert!(!out.status.success());
    assert!(text(&out).contains("download failed"), "{}", text(&out));

    // 7. A payload reporting another version than the tag: refused,
    //    nothing installed.
    sb.release("8.8.8", Payload::Reports("7.7.7"), None);
    let out = sb.run("8.8.8");
    assert!(!out.status.success());
    assert!(text(&out).contains("reports 'scout 7.7.7', not 'scout 8.8.8'"), "{}", text(&out));
    assert_eq!(sb.bin(), real, "binary untouched");

    // 8. A tarball missing a member: refused before the first install.
    sb.release("6.6.6", Payload::Reports("6.6.6"), Some("config.toml"));
    let out = sb.run("6.6.6");
    assert!(!out.status.success());
    assert!(text(&out).contains("no regular file config.toml"), "{}", text(&out));
    assert_eq!(sb.bin(), real, "binary untouched");

    // 9. A version argument that is not a tag never reaches a URL.
    let out = sb.run("../x");
    assert!(!out.status.success());
    assert!(text(&out).contains("not a release tag"), "{}", text(&out));

    // 10. Recon, run from the installed layout, sees the installed
    //     wrapper: widen it and the finding names it.
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o666)).unwrap();
    let scout = |args: &[&str]| {
        Command::new(sb.path("prefix/bin/scout"))
            .args(args)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", sb.path("home"))
            .env("XDG_CONFIG_HOME", sb.path("home/.config"))
            .env("XDG_DATA_HOME", sb.path("home/.local/share"))
            .env("XDG_STATE_HOME", sb.path("home/.local/state"))
            .output()
            .unwrap()
    };
    let out = scout(&["index", sb.path("tree").to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out));
    let out = scout(&["recon", "--format", "tsv"]);
    let report = text(&out);
    assert!(
        report.contains("own-wrapper-writable"),
        "recon must see the installed wrapper:\n{report}"
    );
    assert!(report.contains("scout-dist/scout.bash"), "{report}");
    assert!(
        !report.contains("own-state-mode"),
        "the install must not widen scout's own dirs:\n{report}"
    );
}
