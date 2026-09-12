//! The canonical projection: the exact byte sequence the trust hash
//! consumes. Hand-emitted rather than serde-emitted, because determinism
//! is the contract and the escape rules are fixed here, not by a
//! library's choices. Descriptions are dropped: they are displayed, never
//! executed, so editing one must not re-prompt.
//!
//! What enters the hash follows one principle: a field is hashed if
//! changing it changes what runs, whether it runs, or which gesture runs
//! it. `when` decides whether an action is offered; `[keys]` decides which
//! keystroke spawns a pane; both are in. `[scout] session` is out: like
//! the selected path or a marker file, it is an input that selects among
//! actions the user has already approved (a `mode` clause is hashed, so
//! both twins of a key are shown at approval), not a field of any action.

use std::collections::BTreeMap;

use crate::actions::{Action, Step, Template};
use crate::platform::hash;

/// Bumped whenever the projection changes shape, so every existing trust
/// entry is invalidated at once and every user re-approves once.
pub const HASH_HEADER: &str = "scout/trust-hash-v2\nschema_version=2\n";

/// Project the user action set (compiled defaults must already be
/// excluded by the caller) and the `[keys]` table into canonical JSON:
/// actions sorted by name bytes, fields in fixed order, placeholders
/// literal, `\n` between top-level action objects, single trailing `\n`.
pub fn projection(user_actions: &[Action], keys: &BTreeMap<String, String>) -> String {
    let mut sorted: Vec<&Action> = user_actions.iter().collect();
    sorted.sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));

    let mut out = String::from("[");
    for (i, action) in sorted.iter().enumerate() {
        if i > 0 {
            out.push_str(",\n");
        }
        out.push_str("{\"name\":");
        json_string(&mut out, &action.name);
        out.push_str(",\"keybinding\":");
        match &action.keybinding {
            Some(k) => json_string(&mut out, k),
            None => out.push_str("null"),
        }
        out.push_str(",\"on_failure\":");
        json_string(&mut out, action.on_failure.as_str());
        out.push_str(",\"unsafe_shell_template\":");
        out.push_str(if action.unsafe_shell_template { "true" } else { "false" });
        out.push_str(",\"steps\":[");
        for (j, step) in action.steps.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            emit_step(&mut out, step);
        }
        out.push_str("],\"when\":");
        emit_when(&mut out, action);
        out.push('}');
    }
    out.push(']');
    // The `[keys]` table, sorted by operation name; an empty table hashes
    // as `{}` so a config with no table and one with an empty table agree.
    out.push_str("\n{\"keys\":{");
    for (i, (op, key)) in keys.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        json_string(&mut out, op);
        out.push(':');
        json_string(&mut out, key);
    }
    out.push_str("}}\n");
    out
}

/// Header plus projection: the full SHA-256 input.
pub fn hash_input(user_actions: &[Action], keys: &BTreeMap<String, String>) -> String {
    format!("{HASH_HEADER}{}", projection(user_actions, keys))
}

pub fn trust_hash(user_actions: &[Action], keys: &BTreeMap<String, String>) -> String {
    hash::hex_digest(hash_input(user_actions, keys).as_bytes())
}

/// `null` when absent; otherwise an object with keys in sorted order and
/// only the keys present. `marker` is always an array. The glob is the
/// pattern as written (before `~` expansion) so the hash is the same on
/// every machine.
fn emit_when(out: &mut String, action: &Action) {
    let Some(when) = &action.when else {
        out.push_str("null");
        return;
    };
    let mut fields: Vec<String> = Vec::new();
    if !when.ext.is_empty() {
        let mut s = String::from("\"ext\":[");
        for (i, e) in when.ext.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            json_string(&mut s, e);
        }
        s.push(']');
        fields.push(s);
    }
    if let Some(finding) = &when.finding {
        let mut s = String::from("\"finding\":");
        json_string(&mut s, finding);
        fields.push(s);
    }
    if let Some(glob) = &when.glob {
        let mut s = String::from("\"glob\":");
        json_string(&mut s, glob);
        fields.push(s);
    }
    if let Some(kind) = when.kind {
        let mut s = String::from("\"kind\":");
        json_string(&mut s, kind.as_str());
        fields.push(s);
    }
    if !when.marker.is_empty() {
        let mut s = String::from("\"marker\":[");
        for (i, m) in when.marker.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            json_string(&mut s, m);
        }
        s.push(']');
        fields.push(s);
    }
    if let Some(mode) = when.mode {
        let mut s = String::from("\"mode\":");
        json_string(&mut s, mode.as_str());
        fields.push(s);
    }
    out.push('{');
    out.push_str(&fields.join(","));
    out.push('}');
}

fn emit_step(out: &mut String, step: &Step) {
    match step {
        Step::Spawn { argv, wait, cwd, pause, pane } => {
            out.push_str("{\"kind\":\"spawn\",\"argv\":[");
            for (i, element) in argv.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                json_string(out, &element.raw);
            }
            out.push_str("],\"wait\":");
            out.push_str(if *wait { "true" } else { "false" });
            out.push_str(",\"cwd\":");
            match cwd {
                Some(template) => json_string(out, &template.raw),
                None => out.push_str("null"),
            }
            out.push_str(",\"pause\":");
            out.push_str(if *pause { "true" } else { "false" });
            out.push_str(",\"pane\":");
            match pane {
                Some(op) => json_string(out, op.as_str()),
                None => out.push_str("null"),
            }
            out.push('}');
        }
        Step::Print { format } => {
            out.push_str("{\"kind\":\"print\",\"format\":");
            json_string(out, &format.raw);
            out.push('}');
        }
        Step::Env { set } => {
            out.push_str("{\"kind\":\"env\",\"set\":{");
            let mut sorted: Vec<&(String, Template)> = set.iter().collect();
            sorted.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            for (i, (name, value)) in sorted.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                json_string(out, name);
                out.push(':');
                json_string(out, &value.raw);
            }
            out.push_str("}}");
        }
        // Compiled defaults never reach the projection; one here is a
        // caller bug worth failing loudly on.
        Step::BuiltinEdit => unreachable!("compiled default hashed"),
    }
}

/// Minimal-escape JSON string: `\"`, `\\`, control bytes as `\u00XX`,
/// everything else literal UTF-8.
fn json_string(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
