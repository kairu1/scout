//! The staged loader. Every stage is a hard gate; a failure at any stage
//! refuses the file. A parse failure halts: it never falls through to a
//! lower-precedence config, because the config the user is editing is
//! the one they expect to load.

use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::keys::{normalise_chord, Keys, Operation, PICKER_OWNED_KEYS};
use super::trust::{TrustStatus, TrustStore};
use super::{canonical, merge_with_defaults, trust, Config};
use crate::actions::template::is_posix_env_name;
use crate::actions::{Action, Kind, OnFailure, PaneOp, Step, Template, When};
use crate::platform::fs as pfs;
use crate::{Error, Result};

const SIZE_CAP: usize = 256 * 1024;
pub const SUPPORTED_SCHEMA: i64 = 2;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    schema_version: i64,
    #[serde(default)]
    scout: Option<toml::Table>,
    #[serde(default)]
    keys: Option<toml::Table>,
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
    #[serde(default)]
    when: Option<RawWhen>,
}

/// The `when` clause as written. Unknown keys refuse the file.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWhen {
    #[serde(default)]
    kind: Option<String>,
    /// A string or an array of strings.
    #[serde(default)]
    marker: Option<toml::Value>,
    #[serde(default)]
    ext: Option<Vec<String>>,
    #[serde(default)]
    glob: Option<String>,
    #[serde(default)]
    finding: Option<String>,
}

/// Chords the picker itself uses; an action may not claim them. Only
/// what is genuinely handled belongs here: reserving a name that nothing
/// handles protects nothing and costs users a usable chord.
pub const PICKER_OWNED: [&str; 1] = ["ctrl-c"];

/// Full load: discovery, gates, trust, merge. `interactive` says whether
/// a trust prompt may be rendered; a required prompt without a terminal
/// refuses.
pub fn load(chain: &[PathBuf], trust_store_path: PathBuf, interactive: bool) -> Result<Config> {
    let Some(config_path) = discover(chain)? else {
        return Ok(Config::builtin_only());
    };
    load_file(&config_path, trust_store_path, interactive)
}

/// Discovery: the first entry whose final component opens as a regular
/// file without following a symlink. Symlinks and absent files fall
/// through; anything that opens is taken. Public so `doctor` can ask
/// which link wins rather than re-deriving the rules.
pub fn discover(chain: &[PathBuf]) -> Result<Option<PathBuf>> {
    for candidate in chain {
        match pfs::open_nofollow(candidate) {
            Ok(Some(_)) => return Ok(Some(candidate.clone())),
            Ok(None) => continue,
            Err(err) => return Err(err.into()),
        }
    }
    Ok(None)
}

pub fn load_file(
    config_path: &Path,
    trust_store_path: PathBuf,
    interactive: bool,
) -> Result<Config> {
    // Open without following symlinks, cap at 256 KiB.
    let mut file = pfs::open_nofollow(config_path)?.ok_or_else(|| {
        Error::ConfigRefused(format!("{}: not a regular file", config_path.display()))
    })?;
    let mut buf = Vec::with_capacity(8 * 1024);
    file.by_ref().take(SIZE_CAP as u64 + 1).read_to_end(&mut buf)?;
    if buf.len() > SIZE_CAP {
        return Err(Error::ConfigRefused(format!(
            "{}: exceeds the 256 KiB config cap",
            config_path.display()
        )));
    }
    let text = String::from_utf8(buf)
        .map_err(|_| Error::ConfigRefused(format!("{}: not valid UTF-8", config_path.display())))?;

    // Parse (toml errors carry line and column in Display).
    let raw: RawConfig = toml::from_str(&text).map_err(|err| Error::ConfigToml {
        path: config_path.to_path_buf(),
        message: err.to_string(),
    })?;

    // Schema version.
    if raw.schema_version != SUPPORTED_SCHEMA {
        return Err(Error::ConfigSchemaVersion {
            path: config_path.to_path_buf(),
            found: raw.schema_version,
            supported: SUPPORTED_SCHEMA,
        });
    }
    // `[scout]` carries exactly one setting.
    let mut session = false;
    if let Some(scout_table) = &raw.scout {
        for (key, value) in scout_table {
            match (key.as_str(), value) {
                ("session", toml::Value::Boolean(b)) => session = *b,
                ("session", _) => {
                    return Err(validation(config_path, "[scout] session must be a boolean".into()))
                }
                (other, _) => {
                    return Err(validation(
                        config_path,
                        format!("[scout]: unknown key `{other}` (the only setting is `session`)"),
                    ))
                }
            }
        }
    }
    // `[keys]`: named operations on keys the user chose. Validated even
    // outside tmux; it is about the file, not the environment.
    let mut explicit: BTreeMap<Operation, String> = BTreeMap::new();
    if let Some(table) = &raw.keys {
        for (name, value) in table {
            let op = Operation::parse(name).ok_or_else(|| {
                validation(
                    config_path,
                    format!(
                        "[keys]: unknown operation `{name}` (want one of {})",
                        super::keys::ALL_OPERATIONS
                            .iter()
                            .map(|o| o.name())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                )
            })?;
            let Some(text) = value.as_str() else {
                return Err(validation(config_path, format!("[keys] {name} must be a string")));
            };
            let chord = normalise_chord(text).ok_or_else(|| {
                validation(
                    config_path,
                    format!(
                        "[keys] {name} = `{text}` is not a key: use <mods>-<key> with ctrl, alt, shift \
                         and a letter, f1-f12, an arrow, home, end, pageup or pagedown"
                    ),
                )
            })?;
            if PICKER_OWNED_KEYS.contains(&chord.as_str()) {
                return Err(validation(
                    config_path,
                    format!("[keys] {name} = `{chord}` is a key the picker itself uses"),
                ));
            }
            if let Some((other, _)) = explicit.iter().find(|(_, k)| **k == chord) {
                return Err(validation(
                    config_path,
                    format!("[keys] {name} and {} both use `{chord}`", other.name()),
                ));
            }
            explicit.insert(op, chord);
        }
    }
    // The hashed form: only what the user wrote.
    let keys: BTreeMap<String, String> =
        explicit.iter().map(|(op, k)| (op.name().to_string(), k.clone())).collect();

    // Typed validation of every action.
    let mut warnings = Vec::new();
    let mut actions = Vec::with_capacity(raw.actions.len());
    for raw_action in &raw.actions {
        actions.push(validate_action(config_path, raw_action, &mut warnings)?);
    }
    let mut names = HashSet::new();
    for action in &actions {
        if !names.insert(action.name.clone()) {
            return Err(validation(
                config_path,
                format!("duplicate action name `{}`", action.name),
            ));
        }
    }
    // Two actions on one chord has no correct resolution: choosing either
    // makes the other silently dead.
    let mut seen: Vec<&str> = Vec::new();
    for action in &actions {
        if let Some(binding) = action.keybinding.as_deref() {
            if binding != "enter" && seen.contains(&binding) {
                return Err(validation(
                    config_path,
                    format!("keybinding `{binding}` is claimed by more than one action"),
                ));
            }
            seen.push(binding);
        }
    }

    let enter_count = actions.iter().filter(|a| a.keybinding.as_deref() == Some("enter")).count();
    if enter_count > 1 {
        return Err(validation(
            config_path,
            "more than one action binds `enter` (dispatch ambiguity)".into(),
        ));
    }
    // An explicit `[keys]` entry may not take an action's chord; a default
    // that would is dropped with a warning (the action was there first).
    let action_chords: Vec<String> =
        actions.iter().filter_map(|a| a.keybinding.clone()).filter(|k| k != "enter").collect();
    for (op, chord) in &explicit {
        if action_chords.contains(chord) {
            return Err(validation(
                config_path,
                format!("[keys] {} = `{chord}` is already an action's keybinding", op.name()),
            ));
        }
    }
    let resolved_keys = Keys::resolve(&explicit, &action_chords, &mut warnings);

    // Canonical projection, hash, trust.
    let hash = canonical::trust_hash(&actions, &keys);
    let mut store = TrustStore::load(trust_store_path)?;
    let status = store.status(config_path, &hash);
    match status {
        TrustStatus::Trusted => {}
        TrustStatus::New | TrustStatus::Changed { .. } => {
            if !(interactive && trust::tty_available()) {
                return Err(Error::TrustRequiresTty {
                    config: config_path.to_path_buf(),
                    hash,
                    store: store.store_path().to_path_buf(),
                });
            }
            if !trust::prompt(config_path, &actions, &keys, &status)? {
                return Err(Error::TrustDeclined);
            }
            store.record(config_path, &hash)?;
        }
    }

    // Merge with compiled defaults, user wins by name.
    Ok(Config {
        actions: merge_with_defaults(actions),
        warnings,
        source: Some(config_path.to_path_buf()),
        trust_hash: Some(hash),
        session,
        keys: resolved_keys,
    })
}

fn validation(path: &Path, message: String) -> Error {
    Error::ConfigInvalid { path: path.to_path_buf(), message }
}

fn validate_action(path: &Path, raw: &RawAction, warnings: &mut Vec<String>) -> Result<Action> {
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

    // Normalised once, here: dispatch lower-cases the pressed key, so a
    // binding written `ctrl-C` would otherwise load cleanly and never fire.
    let keybinding = raw.keybinding.as_deref().map(str::to_ascii_lowercase);
    if let Some(binding) = keybinding.as_deref() {
        if binding != "enter" {
            let chord = binding
                .strip_prefix("alt-")
                .or_else(|| binding.strip_prefix("ctrl-"))
                .map(|rest| rest.len() == 1 && rest.chars().all(|c| c.is_ascii_alphabetic()))
                .unwrap_or(false);
            if binding == "tab" {
                // Permanent: `tab` opens the action pane, the only route to
                // every action without a binding.
                warnings.push(format!(
                    "action `{name}`: `tab` is reserved for the action pane and will not \
                     dispatch; use an alt- or ctrl- chord"
                ));
            } else if PICKER_OWNED.contains(&binding) {
                return Err(validation(
                    path,
                    format!(
                        "action `{name}`: binding `{binding}` is reserved by the picker; \
                         choose another chord"
                    ),
                ));
            } else if !chord {
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

    let parse_template = |field: &str, value: &str| -> Result<Template> {
        Template::parse(value)
            .map_err(|e| validation(path, format!("action `{name}`, {field}: {e}")))
    };

    // The description is template-checked against the closed set but only
    // ever displayed.
    if let Some(description) = &raw.description {
        parse_template("description", description)?;
    }

    let mut steps = Vec::with_capacity(raw.steps.len());
    for (index, table) in raw.steps.iter().enumerate() {
        steps.push(validate_step(path, name, index, table, &parse_template)?);
    }

    let when = match &raw.when {
        Some(raw_when) => Some(validate_when(path, name, raw_when)?),
        None => None,
    };

    let unsafe_shell_template = raw.unsafe_shell_template.unwrap_or(false);
    // The `sh -c` shape is checked first: its payload is the one seam
    // where placeholder-plus-prose is legal, bought by the attestation;
    // the single-slot rule governs every other argv element.
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
        // An audit trail for actions that cleared the ceremonial gate.
        tracing::warn!(action = %name, "config.unsafe_shell_template action loaded");
    }

    Ok(Action {
        name: name.clone(),
        description: raw.description.clone().unwrap_or_default(),
        keybinding,
        on_failure,
        unsafe_shell_template,
        steps,
        when,
        from_user_config: true,
    })
}

/// A file name a marker may name: one path component, no separators.
fn is_marker_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 255 && !name.contains('/') && name != "." && name != ".."
}

/// A recon check name: lowercase words joined by hyphens.
fn is_check_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().next().map(|b| b.is_ascii_lowercase()).unwrap_or(false)
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn validate_when(path: &Path, action: &str, raw: &RawWhen) -> Result<When> {
    let bad = |message: String| validation(path, format!("action `{action}`, when: {message}"));

    let kind = match raw.kind.as_deref() {
        None => None,
        Some(k) => {
            Some(Kind::parse(k).ok_or_else(|| bad(format!("kind `{k}` (want repo|dir|file)")))?)
        }
    };
    let marker: Vec<String> = match &raw.marker {
        None => Vec::new(),
        Some(toml::Value::String(s)) => vec![s.clone()],
        Some(toml::Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    toml::Value::String(s) => out.push(s.clone()),
                    _ => return Err(bad("`marker` entries must be strings".into())),
                }
            }
            out
        }
        Some(_) => return Err(bad("`marker` must be a string or an array of strings".into())),
    };
    for m in &marker {
        if !is_marker_name(m) {
            return Err(bad(format!("marker `{m}` must be a bare file name")));
        }
    }
    if raw.marker.is_some() && marker.is_empty() {
        return Err(bad("`marker` must name at least one file".into()));
    }
    let ext = raw.ext.clone().unwrap_or_default();
    for e in &ext {
        if e.is_empty() || e.starts_with('.') || e.contains('/') {
            return Err(bad(format!("ext `{e}` must be an extension without the dot")));
        }
    }
    if raw.ext.is_some() && ext.is_empty() {
        return Err(bad("`ext` must name at least one extension".into()));
    }
    if let Some(g) = &raw.glob {
        if g.is_empty() {
            return Err(bad("`glob` must not be empty".into()));
        }
    }
    if let Some(f) = &raw.finding {
        if !is_check_name(f) {
            return Err(bad(format!(
                "finding `{f}` must be a check name like `world-writable-dir`"
            )));
        }
    }
    if kind.is_none()
        && marker.is_empty()
        && ext.is_empty()
        && raw.glob.is_none()
        && raw.finding.is_none()
    {
        return Err(bad("an empty `when = {}` means always; omit it instead".into()));
    }
    let home = crate::platform::xdg::home().map(|h| h.display().to_string()).unwrap_or_default();
    When::new(kind, marker, ext, raw.glob.clone(), raw.finding.clone(), &home).map_err(bad)
}

fn validate_step(
    path: &Path,
    action: &str,
    index: usize,
    table: &toml::Table,
    parse_template: &dyn Fn(&str, &str) -> Result<Template>,
) -> Result<Step> {
    let bad =
        |message: String| validation(path, format!("action `{action}`, step {index}: {message}"));

    let kind = table
        .get("kind")
        .and_then(|v| v.as_str())
        .ok_or_else(|| bad("missing string field `kind`".into()))?;

    let allowed: &[&str] = match kind {
        "spawn" => &["kind", "argv", "wait", "cwd", "pause", "pane"],
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
            let argv_value =
                table.get("argv").ok_or_else(|| bad("spawn requires `argv`".into()))?;
            let argv_list = argv_value.as_array().ok_or_else(|| {
                bad("`argv` must be an array of strings (single-string argv is refused)".into())
            })?;
            if argv_list.is_empty() {
                return Err(bad("`argv` must have at least one element".into()));
            }
            let mut argv = Vec::with_capacity(argv_list.len());
            for (i, element) in argv_list.iter().enumerate() {
                let s =
                    element.as_str().ok_or_else(|| bad(format!("argv[{i}] must be a string")))?;
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
            let pause = match table.get("pause") {
                None => true,
                Some(toml::Value::Boolean(b)) => *b,
                Some(_) => return Err(bad("`pause` must be a boolean".into())),
            };
            let pane = match table.get("pane") {
                None => None,
                Some(toml::Value::String(s)) => Some(PaneOp::parse(s).ok_or_else(|| {
                    bad(format!("pane `{s}` (want split-right|split-down|new-window)"))
                })?),
                Some(_) => return Err(bad("`pane` must be a string".into())),
            };
            if pane.is_some() {
                // A pane is not waited for and cannot pause: scout stays
                // in its own pane while the command runs in the new one.
                if table.get("wait").is_some_and(|w| w.as_bool() == Some(true)) {
                    return Err(bad("`pane` cannot be combined with `wait = true`".into()));
                }
                if table.get("pause").is_some() {
                    return Err(bad("`pane` cannot be combined with `pause`".into()));
                }
            }
            Ok(Step::Spawn { argv, wait, cwd, pause, pane })
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
                if !is_posix_env_name(env_name) {
                    return Err(bad(format!("env name `{env_name}` violates POSIX convention")));
                }
                let value = value
                    .as_str()
                    .ok_or_else(|| bad(format!("env value for `{env_name}` must be a string")))?;
                bindings
                    .push((env_name.clone(), parse_template(&format!("set.{env_name}"), value)?));
            }
            Ok(Step::Env { set: bindings })
        }
        _ => unreachable!(),
    }
}
