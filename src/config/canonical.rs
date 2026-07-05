//! Canonical-JSON projection (ADR-004 §9) — the exact byte sequence the
//! trust hash consumes. Hand-emitted, not serde-emitted: determinism is
//! the contract and the escape rules are fixed here, not by a library's
//! choices.

use super::{Action, Step};

pub const HASH_HEADER: &str = "scout/trust-hash-v1\nschema_version=1\n";

/// Project the USER action set (compiled defaults must already be
/// excluded by the caller) into canonical JSON. Descriptions dropped,
/// actions sorted by name bytes, fields in fixed order, placeholders
/// literal, `\n` between top-level action objects, single trailing `\n`.
pub fn projection(user_actions: &[Action]) -> String {
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
        out.push_str("]}");
    }
    out.push_str("]\n");
    out
}

/// Header + projection: the full SHA-256 input (ADR-004 §9 step 9-10).
pub fn hash_input(user_actions: &[Action]) -> String {
    format!("{HASH_HEADER}{}", projection(user_actions))
}

pub fn trust_hash(user_actions: &[Action]) -> String {
    super::sha256::hex_digest(hash_input(user_actions).as_bytes())
}

fn emit_step(out: &mut String, step: &Step) {
    match step {
        Step::Spawn { argv, wait, cwd } => {
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
            out.push('}');
        }
        Step::Print { format } => {
            out.push_str("{\"kind\":\"print\",\"format\":");
            json_string(out, &format.raw);
            out.push('}');
        }
        Step::Env { set } => {
            out.push_str("{\"kind\":\"env\",\"set\":{");
            let mut sorted: Vec<&(String, super::template::Template)> = set.iter().collect();
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
        // Compiled defaults never reach the projection; a BuiltinEdit
        // here is a caller bug worth failing loudly on.
        Step::BuiltinEdit => unreachable!("compiled default hashed"),
    }
}

/// Minimal-escape JSON string per ADR-004 §9 step 8: `\"`, `\\`, and
/// control bytes as `\u00XX`; everything else literal UTF-8.
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
