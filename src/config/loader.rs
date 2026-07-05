//! Staged config loader (ADR-004 §10). Every stage is a hard gate; a
//! parse failure halts — it never falls through to a lower-precedence
//! config (ADR-004 §8).

use std::collections::HashSet;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::template::Template;
use super::trust::{TrustStatus, TrustStore};
use super::{canonical, merge_with_defaults, trust, Action, Config, OnFailure, Step};

const SIZE_CAP: usize = 256 * 1024;

#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "x86")))]
const O_NOFOLLOW: i32 = 0o400000;
#[cfg(all(target_os = "linux", not(any(target_arch = "x86_64", target_arch = "x86"))))]
const O_NOFOLLOW: i32 = 0o100000;
#[cfg(target_os = "macos")]
const O_NOFOLLOW: i32 = 0x0100;

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("config refused: {0}")]
    Refused(String),
    #[error("toml parse error in {path}: {message}")]
    Toml { path: PathBuf, message: String },
    #[error("{path}: schema_version {found} not supported; this binary supports 1")]
    SchemaVersion { path: PathBuf, found: i64 },
    #[error("invalid config in {path}: {message}")]
    Validation { path: PathBuf, message: String },
    #[error("config not trusted: declined at prompt")]
    TrustDeclined,
    #[error(
        "config at {config} requires an interactive trust decision but no TTY is attached; \
         run scout interactively once, or verify hash {hash} against trust store {store}"
    )]
    NonTtyUntrusted { config: PathBuf, hash: String, store: PathBuf },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    schema_version: i64,
    #[serde(default)]
    scout: Option<toml::Table>,
    #[serde(default, rename = "action")]
    actions: Vec<RawAction>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAction {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    keybinding: Option<String>,
    #[serde(default)]
    on_failure: Option<String>,
    #[serde(default)]
    unsafe_shell_template: Option<bool>,
    steps: Vec<toml::Table>,
}

/// Full load: discovery → gates → trust → merge. `interactive` controls
/// whether a trust prompt may be rendered (the caller has already
/// decided the UI mode); a required prompt without a TTY refuses.
pub fn load(chain: &[PathBuf], trust_store_path: PathBuf, interactive: bool) -> Result<Config, LoadError> {
    let Some(config_path) = discover(chain)? else {
        return Ok(Config::builtin_only());
    };
    load_file(&config_path, trust_store_path, interactive)
}

/// Discovery (ADR-004 §8): first entry whose final component opens
/// O_NOFOLLOW as a regular file. Symlinks fall through; absence falls
/// through; anything that opens is taken.
fn discover(chain: &[PathBuf]) -> Result<Option<PathBuf>, LoadError> {
    for candidate in chain {
        match open_nofollow(candidate) {
            Ok(Some(_)) => return Ok(Some(candidate.clone())),
            Ok(None) => continue,
            Err(err) => return Err(err.into()),
        }
    }
    Ok(None)
}

/// Ok(Some(file)) = regular file opened; Ok(None) = fall through
/// (absent or symlink); Err = real IO failure worth surfacing.
fn open_nofollow(path: &Path) -> std::io::Result<Option<std::fs::File>> {
    match std::fs::OpenOptions::new().read(true).custom_flags(O_NOFOLLOW).open(path) {
        Ok(file) => {
            if file.metadata()?.is_file() {
                Ok(Some(file))
            } else {
                Ok(None)
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_)
            if path
                .symlink_metadata()
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false) =>
        {
            Ok(None)
        }
        Err(err) => Err(err),
    }
}

pub fn load_file(
    config_path: &Path,
    trust_store_path: PathBuf,
    interactive: bool,
) -> Result<Config, LoadError> {
    // Stage 2-3: open O_NOFOLLOW, cap at 256 KiB.
    let mut file = open_nofollow(config_path)?
        .ok_or_else(|| LoadError::Refused(format!("{}: not a regular file", config_path.display())))?;
    let mut buf = Vec::with_capacity(8 * 1024);
    file.by_ref().take(SIZE_CAP as u64 + 1).read_to_end(&mut buf)?;
    if buf.len() > SIZE_CAP {
        return Err(LoadError::Refused(format!(
            "{}: exceeds the 256 KiB config cap",
            config_path.display()
        )));
    }
    let text = String::from_utf8(buf).map_err(|_| {
        LoadError::Refused(format!("{}: not valid UTF-8", config_path.display()))
    })?;

    // Stage 4: TOML parse (toml errors carry line/column in Display).
    let raw: RawConfig = toml::from_str(&text).map_err(|err| LoadError::Toml {
        path: config_path.to_path_buf(),
        message: err.to_string(),
    })?;

    // Stage 5: schema version.
    if raw.schema_version != 1 {
        return Err(LoadError::SchemaVersion { path: config_path.to_path_buf(), found: raw.schema_version });
    }
    // [scout] is reserved and must be empty in v1 (ADR-004 §1).
    if let Some(scout_table) = &raw.scout {
        if let Some(key) = scout_table.keys().next() {
            return Err(LoadError::Validation {
                path: config_path.to_path_buf(),
                message: format!("[scout] is reserved in v1; unknown key `{key}`"),
            });
        }
    }

    // Stages 6-10: typed validation.
    let mut warnings = Vec::new();
    let mut actions = Vec::with_capacity(raw.actions.len());
    for raw_action in &raw.actions {
        actions.push(validate_action(config_path, raw_action, &mut warnings)?);
    }
    let mut names = HashSet::new();
    for action in &actions {
        if !names.insert(action.name.clone()) {
            return Err(LoadError::Validation {
                path: config_path.to_path_buf(),
                message: format!("duplicate action name `{}`", action.name),
            });
        }
    }
    let enter_count =
        actions.iter().filter(|a| a.keybinding.as_deref() == Some("enter")).count();
    if enter_count > 1 {
        return Err(LoadError::Validation {
            path: config_path.to_path_buf(),
            message: "more than one action binds `enter` (dispatch ambiguity)".into(),
        });
    }

    // Stage 11-12: canonical projection, hash, trust.
    let hash = canonical::trust_hash(&actions);
    let mut store = TrustStore::load(trust_store_path)?;
    let status = store.status(config_path, &hash);
    match status {
        TrustStatus::Trusted => {}
        TrustStatus::New | TrustStatus::Changed { .. } => {
            if !(interactive && trust::tty_available()) {
                return Err(LoadError::NonTtyUntrusted {
                    config: config_path.to_path_buf(),
                    hash,
                    store: store.store_path().to_path_buf(),
                });
            }
            if !trust::prompt(config_path, &actions, &status)? {
                return Err(LoadError::TrustDeclined);
            }
            store.record(config_path, &hash)?;
        }
    }

    // Stage 13: merge with compiled defaults, user wins by name.
    Ok(Config {
        actions: merge_with_defaults(actions),
        warnings,
        source: Some(config_path.to_path_buf()),
        trust_hash: Some(hash),
    })
}

fn validation(path: &Path, message: String) -> LoadError {
    LoadError::Validation { path: path.to_path_buf(), message }
}

fn validate_action(
    path: &Path,
    raw: &RawAction,
    warnings: &mut Vec<String>,
) -> Result<Action, LoadError> {
    let name = &raw.name;
    let name_ok = !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if !name_ok {
        return Err(validation(
            path,
            format!("action name `{name}` must be ASCII [A-Za-z0-9_-], length 1-64"),
        ));
    }

    let on_failure = match raw.on_failure.as_deref() {
        None | Some("abort") => OnFailure::Abort,
        Some("continue") => OnFailure::Continue,
        Some(other) => {
            return Err(validation(
                path,
                format!("action `{name}`: on_failure `{other}` (want abort|continue)"),
            ))
        }
    };

    if let Some(binding) = raw.keybinding.as_deref() {
        if binding != "enter" {
            let reserved = binding == "tab"
                || binding
                    .strip_prefix("alt-")
                    .or_else(|| binding.strip_prefix("ctrl-"))
                    .map(|rest| rest.len() == 1 && rest.chars().all(|c| c.is_ascii_alphabetic()))
                    .unwrap_or(false);
            if reserved {
                warnings.push(format!(
                    "action `{name}`: binding `{binding}` recognised but not dispatched in v1"
                ));
            } else {
                warnings.push(format!(
                    "action `{name}`: unknown keybinding `{binding}`; it will not dispatch"
                ));
            }
        }
    }

    if raw.steps.is_empty() || raw.steps.len() > 32 {
        return Err(validation(
            path,
            format!("action `{name}`: steps must number 1-32, got {}", raw.steps.len()),
        ));
    }

    let parse_template = |field: &str, value: &str| -> Result<Template, LoadError> {
        Template::parse(value)
            .map_err(|e| validation(path, format!("action `{name}`, {field}: {e}")))
    };

    // Description is template-checked for the closed set but only ever
    // displayed (ADR-004 §4).
    if let Some(description) = &raw.description {
        parse_template("description", description)?;
    }

    let mut steps = Vec::with_capacity(raw.steps.len());
    for (index, table) in raw.steps.iter().enumerate() {
        steps.push(validate_step(path, name, index, table, &parse_template)?);
    }

    let unsafe_shell_template = raw.unsafe_shell_template.unwrap_or(false);
    // Stage 8-9 (ADR-004 §4, ADR-003 §2): the sh -c shape is checked
    // first — its payload is the one seam where placeholder-plus-prose
    // is legal, bought by the attestation; the single-slot rule governs
    // every other argv element.
    for (index, step) in steps.iter().enumerate() {
        if let Step::Spawn { argv, .. } = step {
            let is_shell_dash_c = argv.len() >= 3
                && Path::new(&argv[0].raw)
                    .file_name()
                    .map(|b| matches!(b.to_str(), Some("sh" | "bash" | "dash" | "zsh" | "ksh")))
                    .unwrap_or(false)
                && argv[1].raw == "-c";
            let templated_payload = argv.iter().skip(2).any(|t| t.has_placeholder());
            if is_shell_dash_c && templated_payload && !unsafe_shell_template {
                return Err(validation(
                    path,
                    format!(
                        "action `{name}`, step {index}: sh -c with placeholders requires \
                         unsafe_shell_template = true"
                    ),
                ));
            }
            for (i, element) in argv.iter().enumerate() {
                if is_shell_dash_c && i >= 2 {
                    continue;
                }
                if element.violates_single_slot() {
                    return Err(validation(
                        path,
                        format!(
                            "action `{name}`, step {index}: argv[{i}] `{}` mixes a placeholder \
                             with whitespace or shell metacharacters (single-slot rule)",
                            element.raw
                        ),
                    ));
                }
            }
        }
    }
    if unsafe_shell_template {
        // Audit trail for actions that cleared the ceremonial gate
        // (ADR-003 §2 seam 2).
        tracing::warn!(action = %name, "config.unsafe_shell_template action loaded");
    }

    Ok(Action {
        name: name.clone(),
        description: raw.description.clone().unwrap_or_default(),
        keybinding: raw.keybinding.clone(),
        on_failure,
        unsafe_shell_template,
        steps,
        from_user_config: true,
    })
}

fn validate_step(
    path: &Path,
    action: &str,
    index: usize,
    table: &toml::Table,
    parse_template: &dyn Fn(&str, &str) -> Result<Template, LoadError>,
) -> Result<Step, LoadError> {
    let bad = |message: String| validation(path, format!("action `{action}`, step {index}: {message}"));

    let kind = table
        .get("kind")
        .and_then(|v| v.as_str())
        .ok_or_else(|| bad("missing string field `kind`".into()))?;

    let allowed: &[&str] = match kind {
        "spawn" => &["kind", "argv", "wait", "cwd"],
        "print" => &["kind", "format"],
        "env" => &["kind", "set"],
        other => return Err(bad(format!("unknown step kind `{other}` (want spawn|print|env)"))),
    };
    for key in table.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(bad(format!("field `{key}` is not valid on a `{kind}` step")));
        }
    }

    match kind {
        "spawn" => {
            let argv_value = table.get("argv").ok_or_else(|| bad("spawn requires `argv`".into()))?;
            let argv_list = argv_value
                .as_array()
                .ok_or_else(|| bad("`argv` must be an array of strings (single-string argv is refused)".into()))?;
            if argv_list.is_empty() {
                return Err(bad("`argv` must have at least one element".into()));
            }
            let mut argv = Vec::with_capacity(argv_list.len());
            for (i, element) in argv_list.iter().enumerate() {
                let s = element
                    .as_str()
                    .ok_or_else(|| bad(format!("argv[{i}] must be a string")))?;
                // Single-slot rule enforced in the action-level pass,
                // where the sh -c payload exemption is known.
                argv.push(parse_template(&format!("argv[{i}]"), s)?);
            }
            let wait = match table.get("wait") {
                None => true,
                Some(toml::Value::Boolean(b)) => *b,
                Some(_) => return Err(bad("`wait` must be a boolean".into())),
            };
            let cwd = match table.get("cwd") {
                None => None,
                Some(toml::Value::String(s)) => Some(parse_template("cwd", s)?),
                Some(_) => return Err(bad("`cwd` must be a string".into())),
            };
            Ok(Step::Spawn { argv, wait, cwd })
        }
        "print" => {
            let format = table
                .get("format")
                .and_then(|v| v.as_str())
                .ok_or_else(|| bad("print requires string `format`".into()))?;
            Ok(Step::Print { format: parse_template("format", format)? })
        }
        "env" => {
            let set = table
                .get("set")
                .and_then(|v| v.as_table())
                .ok_or_else(|| bad("env requires table `set`".into()))?;
            if set.is_empty() {
                return Err(bad("env `set` must have at least one entry".into()));
            }
            let mut bindings = Vec::with_capacity(set.len());
            for (env_name, value) in set {
                let name_ok = !env_name.is_empty()
                    && env_name.len() <= 64
                    && env_name
                        .bytes()
                        .next()
                        .map(|b| b.is_ascii_alphabetic() || b == b'_')
                        .unwrap_or(false)
                    && env_name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
                if !name_ok {
                    return Err(bad(format!("env name `{env_name}` violates POSIX convention")));
                }
                let value = value
                    .as_str()
                    .ok_or_else(|| bad(format!("env value for `{env_name}` must be a string")))?;
                bindings.push((env_name.clone(), parse_template(&format!("set.{env_name}"), value)?));
            }
            Ok(Step::Env { set: bindings })
        }
        _ => unreachable!(),
    }
}
