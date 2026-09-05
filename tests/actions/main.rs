//! Tests for the action executor and the template grammar.

mod exec;
mod template;

use std::fs;
use std::path::PathBuf;

use scout::actions::{Action, OnFailure, Step, Template};

pub fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "scout-actions-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn t(raw: &str) -> Template {
    Template::parse(raw).unwrap()
}

pub fn action(name: &str, on_failure: OnFailure, steps: Vec<Step>) -> Action {
    Action {
        name: name.into(),
        description: String::new(),
        keybinding: None,
        on_failure,
        unsafe_shell_template: false,
        steps,
        when: None,
        from_user_config: true,
    }
}
