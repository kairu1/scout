//! The `[keys]` table: plain-word pane operations bound to keys the user
//! chooses, plus the re-index key. The names are scout's vocabulary; the
//! translation to tmux lives in the `tmux` module.
//!
//! One key grammar serves the table: `<mods>-<key>`, where `<mods>` is a
//! `-`-joined subset of `ctrl`, `alt`, `shift` in that order and `<key>` is
//! a letter, `f1`-`f12`, an arrow, `home`, `end`, `pageup` or `pagedown`.
//! Action chords keep their older, narrower form (`alt-` or `ctrl-` plus a
//! letter); widening what an action may bind is a separate decision.

use std::collections::BTreeMap;

/// A named pane operation, or the re-index key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Operation {
    SplitRight,
    SplitDown,
    NewWindow,
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    ClosePane,
    Zoom,
    /// Focus the picker's pane from wherever focus is. Installed as a
    /// tmux binding on scout's own server, so it works from any pane.
    FocusPicker,
    /// Close the pane the key was pressed in. Installed as a tmux binding
    /// on scout's own server; refuses the picker's pane.
    KillPane,
    Reindex,
}

pub const ALL_OPERATIONS: [Operation; 12] = [
    Operation::SplitRight,
    Operation::SplitDown,
    Operation::NewWindow,
    Operation::FocusLeft,
    Operation::FocusRight,
    Operation::FocusUp,
    Operation::FocusDown,
    Operation::ClosePane,
    Operation::Zoom,
    Operation::FocusPicker,
    Operation::KillPane,
    Operation::Reindex,
];

impl Operation {
    pub fn name(&self) -> &'static str {
        match self {
            Operation::SplitRight => "split-right",
            Operation::SplitDown => "split-down",
            Operation::NewWindow => "new-window",
            Operation::FocusLeft => "focus-left",
            Operation::FocusRight => "focus-right",
            Operation::FocusUp => "focus-up",
            Operation::FocusDown => "focus-down",
            Operation::ClosePane => "close-pane",
            Operation::Zoom => "zoom",
            Operation::FocusPicker => "focus-picker",
            Operation::KillPane => "kill-pane",
            Operation::Reindex => "reindex",
        }
    }

    pub fn parse(name: &str) -> Option<Operation> {
        ALL_OPERATIONS.iter().copied().find(|op| op.name() == name)
    }

    /// What the help overlay says.
    pub fn describe(&self) -> &'static str {
        match self {
            Operation::SplitRight => "open a pane to the right at the selection",
            Operation::SplitDown => "open a pane below at the selection",
            Operation::NewWindow => "open a window at the selection",
            Operation::FocusLeft => "focus the pane to the left",
            Operation::FocusRight => "focus the pane to the right",
            Operation::FocusUp => "focus the pane above",
            Operation::FocusDown => "focus the pane below",
            Operation::ClosePane => "close the last pane scout opened",
            Operation::Zoom => "zoom the current pane in or out",
            Operation::FocusPicker => "focus the picker, from any pane",
            Operation::KillPane => "close the pane you are in (never the picker), from any pane",
            Operation::Reindex => "re-index every indexed tree without leaving",
        }
    }

    /// Whether the operation needs tmux. Re-index does not.
    pub fn needs_tmux(&self) -> bool {
        !matches!(self, Operation::Reindex)
    }

    /// The key an operation has when the user says nothing. Alt with an
    /// arrow or a letter, because most terminals deliver those; Alt-Shift
    /// with an arrow for focus. The help overlay's key-test line shows
    /// what a terminal actually sends, for the ones that do not.
    pub fn default_key(&self) -> &'static str {
        match self {
            Operation::SplitRight => "alt-right",
            Operation::SplitDown => "alt-down",
            Operation::NewWindow => "alt-w",
            Operation::FocusLeft => "alt-shift-left",
            Operation::FocusRight => "alt-shift-right",
            Operation::FocusUp => "alt-shift-up",
            Operation::FocusDown => "alt-shift-down",
            Operation::ClosePane => "alt-x",
            Operation::Zoom => "alt-z",
            Operation::FocusPicker => "alt-h",
            Operation::KillPane => "alt-q",
            Operation::Reindex => "ctrl-r",
        }
    }

    /// Operations that also work from a pane other than the picker's,
    /// when scout runs on its own tmux server: installed there as tmux
    /// key bindings so one set of keys serves every pane.
    pub fn works_from_any_pane(&self) -> bool {
        matches!(
            self,
            Operation::FocusLeft
                | Operation::FocusRight
                | Operation::FocusUp
                | Operation::FocusDown
                | Operation::Zoom
                | Operation::FocusPicker
                | Operation::KillPane
        )
    }
}

/// A normalised chord as tmux names the same key: `alt-shift-left` is
/// `M-S-Left`, `ctrl-alt-f5` is `C-M-F5`, `pageup` is `PPage`.
pub fn tmux_key_name(chord: &str) -> Option<String> {
    let chord = normalise_chord(chord)?;
    let mut parts: Vec<&str> = chord.split('-').collect();
    let key = parts.pop()?;
    let mut out = String::new();
    for m in parts {
        out.push_str(match m {
            "ctrl" => "C-",
            "alt" => "M-",
            "shift" => "S-",
            _ => return None,
        });
    }
    let named = match key {
        "left" => "Left",
        "right" => "Right",
        "up" => "Up",
        "down" => "Down",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PPage",
        "pagedown" => "NPage",
        other => {
            if let Some(n) = other.strip_prefix('f') {
                if n.parse::<u8>().is_ok() {
                    out.push('F');
                    out.push_str(n);
                    return Some(out);
                }
            }
            out.push_str(other);
            return Some(out);
        }
    };
    out.push_str(named);
    Some(out)
}

/// Keys the picker itself handles, written in the same normalised form
/// the grammar produces. A `[keys]` entry (or an action chord) may not
/// claim one.
pub const PICKER_OWNED_KEYS: [&str; 14] = [
    "esc",
    "enter",
    "tab",
    "ctrl-c",
    "up",
    "down",
    "left",
    "right",
    "home",
    "end",
    "backspace",
    "delete",
    "?",
    "a",
];

const NAMED_KEYS: [&str; 8] = ["left", "right", "up", "down", "home", "end", "pageup", "pagedown"];

/// Normalise a chord written by the user. `None` when it is not in the
/// grammar. Modifiers are accepted in any order and case and emitted in
/// the canonical order, so `Shift-Alt-Left` and `alt-shift-left` are one
/// binding.
pub fn normalise_chord(text: &str) -> Option<String> {
    let lower = text.trim().to_ascii_lowercase();
    let mut parts: Vec<&str> = lower.split('-').collect();
    let key = parts.pop()?;
    let (mut ctrl, mut alt, mut shift) = (false, false, false);
    for m in parts {
        match m {
            "ctrl" if !ctrl => ctrl = true,
            "alt" if !alt => alt = true,
            "shift" if !shift => shift = true,
            _ => return None,
        }
    }
    let key_ok = (key.len() == 1 && key.chars().all(|c| c.is_ascii_alphabetic()))
        || NAMED_KEYS.contains(&key)
        || (key.starts_with('f') && key[1..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)));
    if !key_ok {
        return None;
    }
    // A letter with no modifier types into the search field; it cannot be
    // a binding. Named keys and function keys may stand alone.
    if key.len() == 1 && !ctrl && !alt {
        return None;
    }
    let mut out = String::new();
    if ctrl {
        out.push_str("ctrl-");
    }
    if alt {
        out.push_str("alt-");
    }
    if shift {
        out.push_str("shift-");
    }
    out.push_str(key);
    Some(out)
}

/// The resolved key table: every operation has exactly one key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keys {
    bindings: BTreeMap<Operation, String>,
}

impl Default for Keys {
    fn default() -> Self {
        Keys {
            bindings: ALL_OPERATIONS.iter().map(|op| (*op, op.default_key().to_string())).collect(),
        }
    }
}

impl Keys {
    /// Build from the user's explicit entries (already normalised and
    /// validated) over the defaults. A default whose key collides with an
    /// action chord is dropped and reported in `warnings`, so a user who
    /// bound `alt-w` to an action before `[keys]` existed keeps it.
    pub fn resolve(
        explicit: &BTreeMap<Operation, String>,
        action_chords: &[String],
        warnings: &mut Vec<String>,
    ) -> Keys {
        let mut bindings = BTreeMap::new();
        for op in ALL_OPERATIONS {
            if let Some(key) = explicit.get(&op) {
                bindings.insert(op, key.clone());
                continue;
            }
            let default = op.default_key().to_string();
            if action_chords.contains(&default) {
                warnings.push(format!(
                    "[keys] {} defaults to {default}, which an action already binds; set it \
                     explicitly to use it",
                    op.name()
                ));
                continue;
            }
            if explicit.values().any(|k| *k == default) {
                // The user moved another operation onto this default.
                continue;
            }
            bindings.insert(op, default);
        }
        Keys { bindings }
    }

    pub fn key_for(&self, op: Operation) -> Option<&str> {
        self.bindings.get(&op).map(String::as_str)
    }

    /// The operation bound to a normalised chord, if any.
    pub fn operation_for(&self, chord: &str) -> Option<Operation> {
        self.bindings.iter().find(|(_, k)| k.as_str() == chord).map(|(op, _)| *op)
    }

    pub fn iter(&self) -> impl Iterator<Item = (Operation, &str)> {
        self.bindings.iter().map(|(op, k)| (*op, k.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_normalise_modifier_order_and_case() {
        assert_eq!(normalise_chord("Shift-Alt-Left").as_deref(), Some("alt-shift-left"));
        assert_eq!(normalise_chord("ALT-W").as_deref(), Some("alt-w"));
        assert_eq!(normalise_chord("ctrl-alt-f5").as_deref(), Some("ctrl-alt-f5"));
        assert_eq!(normalise_chord("pagedown").as_deref(), Some("pagedown"));
        assert_eq!(normalise_chord("alt-right").as_deref(), Some("alt-right"));
    }

    #[test]
    fn chords_outside_the_grammar_are_refused() {
        for bad in ["w", "shift-w", "alt-alt-w", "alt-ww", "meta-w", "alt-f13", "alt-", "", "alt-Ω"]
        {
            assert_eq!(normalise_chord(bad), None, "{bad}");
        }
    }

    #[test]
    fn chords_translate_to_tmux_key_names() {
        for (chord, tmux) in [
            ("alt-shift-left", "M-S-Left"),
            ("ctrl-alt-f5", "C-M-F5"),
            ("alt-h", "M-h"),
            ("pageup", "PPage"),
            ("shift-pagedown", "S-NPage"),
            ("ctrl-alt-shift-up", "C-M-S-Up"),
            ("alt-end", "M-End"),
        ] {
            assert_eq!(tmux_key_name(chord).as_deref(), Some(tmux), "{chord}");
        }
        assert_eq!(tmux_key_name("meta-w"), None);
    }

    #[test]
    fn every_operation_has_a_unique_default_and_round_trips() {
        let mut keys: Vec<&str> = ALL_OPERATIONS.iter().map(|op| op.default_key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), ALL_OPERATIONS.len());
        for op in ALL_OPERATIONS {
            assert_eq!(Operation::parse(op.name()), Some(op));
            assert_eq!(normalise_chord(op.default_key()).as_deref(), Some(op.default_key()));
            assert!(!PICKER_OWNED_KEYS.contains(&op.default_key()));
        }
    }

    #[test]
    fn explicit_entries_win_and_colliding_defaults_yield_with_a_warning() {
        let mut explicit = BTreeMap::new();
        explicit.insert(Operation::Zoom, "alt-q".to_string());
        explicit.insert(Operation::NewWindow, "alt-right".to_string());
        let mut warnings = Vec::new();
        let keys = Keys::resolve(&explicit, &["alt-x".to_string()], &mut warnings);
        assert_eq!(keys.key_for(Operation::Zoom), Some("alt-q"));
        assert_eq!(keys.key_for(Operation::NewWindow), Some("alt-right"));
        assert_eq!(keys.key_for(Operation::SplitRight), None, "its default moved to new-window");
        assert_eq!(keys.key_for(Operation::ClosePane), None, "alt-x belongs to an action");
        assert!(warnings.iter().any(|w| w.contains("close-pane")), "{warnings:?}");
        assert_eq!(keys.operation_for("alt-q"), Some(Operation::Zoom));
        assert_eq!(keys.operation_for("ctrl-r"), Some(Operation::Reindex));
    }
}
