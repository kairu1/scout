//! Compiled-in defaults: what scout does with no config file at all, and
//! the editor fallback chain the `edit` default runs.

use std::collections::HashMap;
use std::path::Path;

use super::{Action, OnFailure, Step, Template};

/// The two default actions. Applied only where the user config does not
/// define the same `name`.
pub fn compiled_defaults() -> Vec<Action> {
    vec![
        Action {
            name: "edit".into(),
            description: "Open in $EDITOR (falls back to $VISUAL, then a vi-family binary on PATH)"
                .into(),
            keybinding: Some("enter".into()),
            on_failure: OnFailure::Abort,
            unsafe_shell_template: false,
            steps: vec![Step::BuiltinEdit],
            from_user_config: false,
        },
        Action {
            name: "print-path".into(),
            description: "Print the selection's absolute path to stdout".into(),
            keybinding: None,
            on_failure: OnFailure::Abort,
            unsafe_shell_template: false,
            // Emits a COMMAND (`printf '%s\n' '<path>'`), not a bare value,
            // so it survives the shell wrapper's eval allowlist. `{path}`
            // is still quoted at the print seam.
            steps: vec![Step::Print {
                format: Template::parse("printf '%s\\n' {path}").expect("static template"),
            }],
            from_user_config: false,
        },
    ]
}

/// `$VISUAL`, then `$EDITOR`, then the first of `vi`, `vim`, `nano` on the
/// given `PATH`. Looks in the map a spawned child would inherit, never in
/// the `env`-step bindings: an editor is something the user exports, not
/// something an action sets.
pub fn resolve_editor(scope: &HashMap<String, String>) -> Option<String> {
    for var in ["VISUAL", "EDITOR"] {
        if let Some(v) = scope.get(var) {
            if !v.is_empty() {
                return Some(v.clone());
            }
        }
    }
    let path = scope.get("PATH").cloned().unwrap_or_default();
    for candidate in ["vi", "vim", "nano"] {
        for dir in path.split(':') {
            if dir.is_empty() {
                continue;
            }
            let full = Path::new(dir).join(candidate);
            if full.is_file() {
                return Some(candidate.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::resolve_editor;
    use std::collections::HashMap;

    /// These test the LOOKUP and never spawn anything. An integration test
    /// that ran the built-in editor for real once launched vim on a CI
    /// runner, which opened a directory listing and blocked until the
    /// six-hour job limit.
    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn visual_wins_then_editor() {
        assert_eq!(
            resolve_editor(&env(&[("VISUAL", "code"), ("EDITOR", "vi")])).as_deref(),
            Some("code")
        );
        assert_eq!(resolve_editor(&env(&[("EDITOR", "vi")])).as_deref(), Some("vi"));
    }

    /// An exported-but-empty variable is not a choice.
    #[test]
    fn empty_values_are_skipped() {
        assert_eq!(
            resolve_editor(&env(&[("VISUAL", ""), ("EDITOR", "nano")])).as_deref(),
            Some("nano")
        );
    }

    /// The PATH fallback finds a vi-family binary without consulting the
    /// process environment.
    #[test]
    fn falls_back_to_a_vi_family_binary_on_path() {
        let dir = std::env::temp_dir().join(format!("scout-editor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("vim");
        std::fs::write(&fake, "#!/bin/sh\nexit 0\n").unwrap();

        let path = dir.display().to_string();
        assert_eq!(resolve_editor(&env(&[("PATH", &path)])).as_deref(), Some("vim"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nothing_resolvable_is_none() {
        assert!(resolve_editor(&env(&[("PATH", "/nonexistent-dir-for-scout-test")])).is_none());
        assert!(resolve_editor(&env(&[])).is_none());
    }
}
