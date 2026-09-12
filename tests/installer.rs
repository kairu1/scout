//! The release installer, driven against a fake release served from a
//! `file://` base: the same command installs, updates, and refuses a
//! tarball whose checksum does not match. Needs `curl`, `tar` and
//! `sha256sum` on PATH (the installer's own requirements); skipped
//! otherwise.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn have(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

fn sandbox() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scout-installer-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    for sub in ["home", "prefix", "dist"] {
        fs::create_dir_all(dir.join(sub)).unwrap();
    }
    dir
}

fn target() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "aarch64-unknown-linux-musl"
    } else {
        "x86_64-unknown-linux-musl"
    }
}

/// Lay out `dist/v<version>/<name>.tar.gz` and its `.sha256` exactly as
/// the release workflow does, with the test binary as the payload.
fn fake_release(dir: &Path, version: &str) -> String {
    let name = format!("scout-{version}-{}", target());
    let release = dir.join("dist").join(format!("v{version}"));
    let payload = release.join(&name);
    fs::create_dir_all(&payload).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_scout"), payload.join("scout")).unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("shell/scout.bash"),
        payload.join("scout.bash"),
    )
    .unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/config.toml"),
        payload.join("config.toml"),
    )
    .unwrap();
    fs::write(payload.join("README.md"), "fake\n").unwrap();
    let tar = Command::new("tar")
        .current_dir(&release)
        .args(["czf", &format!("{name}.tar.gz"), &name])
        .status()
        .unwrap();
    assert!(tar.success());
    let sum = Command::new("sha256sum")
        .current_dir(&release)
        .arg(format!("{name}.tar.gz"))
        .output()
        .unwrap();
    fs::write(release.join(format!("{name}.tar.gz.sha256")), sum.stdout).unwrap();
    fs::remove_dir_all(&payload).unwrap();
    name
}

fn run(dir: &Path, version: &str) -> Output {
    Command::new("sh")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("install-release.sh"))
        .arg(version)
        .env("HOME", dir.join("home"))
        .env("PREFIX", dir.join("prefix"))
        .env("SCOUT_RC", dir.join("home/.bashrc"))
        .env("SCOUT_RELEASE_BASE", format!("file://{}", dir.join("dist").display()))
        .env("XDG_CONFIG_HOME", dir.join("home/.config"))
        .output()
        .unwrap()
}

#[test]
fn installs_updates_in_place_and_refuses_a_bad_checksum() {
    if !(have("curl") && have("tar") && have("sha256sum")) {
        eprintln!("curl, tar or sha256sum missing; skipping");
        return;
    }
    let dir = sandbox();
    let name = fake_release(&dir, "9.9.9");

    // 1. First install: binary, snippet, reference config, rc block once.
    let out = run(&dir, "9.9.9");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(out.status.success(), "{text}\n{}", String::from_utf8_lossy(&out.stderr));
    let bin = dir.join("prefix/bin/scout");
    assert!(bin.exists() && dir.join("prefix/share/scout/scout.bash").exists());
    assert!(dir.join("prefix/share/scout/config.toml").exists());
    assert!(!dir.join("home/.config/scout/config.toml").exists(), "never writes a config");
    let rc = fs::read_to_string(dir.join("home/.bashrc")).unwrap();
    let marker = scout::recon::checks::WRAPPER_MARKER;
    assert_eq!(rc.matches(marker).count(), 1, "{rc}");
    assert!(rc.contains("prefix/share/scout/scout.bash"), "{rc}");
    assert!(text.contains("installed: scout "), "{text}");
    assert!(text.contains("no config yet"), "{text}");
    assert!(!dir.join("dist/v9.9.9").join(&name).exists(), "no extracted tree left in dist");

    // 2. Run again: an update in place; the rc block is not repeated.
    let out = run(&dir, "9.9.9");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let rc = fs::read_to_string(dir.join("home/.bashrc")).unwrap();
    assert_eq!(rc.matches(marker).count(), 1, "{rc}");
    assert!(text.contains("already in"), "{text}");

    // 3. A tarball that does not match its checksum installs nothing.
    let sums = dir.join("dist/v9.9.9").join(format!("{name}.tar.gz.sha256"));
    let bad = fs::read_to_string(&sums).unwrap().replacen(|c: char| c.is_ascii_hexdigit(), "0", 4);
    fs::write(&sums, bad).unwrap();
    let before = fs::metadata(&bin).unwrap().modified().unwrap();
    let out = run(&dir, "9.9.9");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("checksum mismatch"));
    assert_eq!(fs::metadata(&bin).unwrap().modified().unwrap(), before, "binary untouched");

    // 4. A version that does not exist fails before touching anything.
    let out = run(&dir, "0.0.0");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("download failed"));

    fs::remove_dir_all(&dir).unwrap();
}
