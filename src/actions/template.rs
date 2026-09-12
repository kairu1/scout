//! The template grammar: a closed set of placeholders, `{{`/`}}` escapes,
//! unknown placeholder = parse error at load, and the single-slot rule for
//! argv elements. Expansion implements the two shell seams exactly:
//! POSIX single-quoting of every placeholder happens only in `print`
//! format strings, because that is the only line a shell will parse.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Every fixed placeholder name `parse` accepts (`env.NAME` is the one
/// open form). The docs and the reference config quote this list; a
/// parity test holds them to it, and a unit test holds `parse` to it.
pub const PLACEHOLDER_NAMES: &[&str] =
    &["path", "name", "parent", "dir", "ext", "repo_root", "home", "query"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placeholder {
    Path,
    Name,
    Parent,
    /// The selection when it is a directory, else its parent: where a
    /// command about the selection runs.
    Dir,
    Ext,
    RepoRoot,
    Home,
    Query,
    Env(String),
}

impl Placeholder {
    fn parse(name: &str) -> Result<Self, String> {
        match name {
            "path" => Ok(Placeholder::Path),
            "name" => Ok(Placeholder::Name),
            "parent" => Ok(Placeholder::Parent),
            "dir" => Ok(Placeholder::Dir),
            "ext" => Ok(Placeholder::Ext),
            "repo_root" => Ok(Placeholder::RepoRoot),
            "home" => Ok(Placeholder::Home),
            "query" => Ok(Placeholder::Query),
            _ => {
                if let Some(env_name) = name.strip_prefix("env.") {
                    if is_posix_env_name(env_name) {
                        return Ok(Placeholder::Env(env_name.to_string()));
                    }
                    return Err(format!("malformed env placeholder name `{env_name}`"));
                }
                Err(format!("unknown placeholder `{{{name}}}`"))
            }
        }
    }
}

/// `[A-Za-z_][A-Za-z0-9_]{0,63}`: the names a POSIX shell will export.
pub fn is_posix_env_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().next().map(|b| b.is_ascii_alphabetic() || b == b'_').unwrap_or(false)
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Literal(String),
    Placeholder(Placeholder),
}

#[derive(Debug, Clone)]
pub struct Template {
    pub raw: String,
    pub segments: Vec<Segment>,
}

impl Template {
    /// Parse at config load. Unknown placeholders and malformed braces
    /// refuse the file.
    pub fn parse(raw: &str) -> Result<Template, String> {
        let mut segments = Vec::new();
        let mut literal = String::new();
        let mut chars = raw.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' => {
                    if chars.peek() == Some(&'{') {
                        chars.next();
                        literal.push('{');
                        continue;
                    }
                    let mut name = String::new();
                    let mut closed = false;
                    for inner in chars.by_ref() {
                        if inner == '}' {
                            closed = true;
                            break;
                        }
                        if inner == '{' {
                            return Err(format!("nested `{{` inside placeholder in `{raw}`"));
                        }
                        name.push(inner);
                    }
                    if !closed {
                        return Err(format!("unterminated `{{` in `{raw}`"));
                    }
                    if !literal.is_empty() {
                        segments.push(Segment::Literal(std::mem::take(&mut literal)));
                    }
                    segments.push(Segment::Placeholder(Placeholder::parse(&name)?));
                }
                '}' => {
                    if chars.peek() == Some(&'}') {
                        chars.next();
                        literal.push('}');
                    } else {
                        return Err(format!("bare `}}` in `{raw}` (escape as `}}}}`)"));
                    }
                }
                other => literal.push(other),
            }
        }
        if !literal.is_empty() {
            segments.push(Segment::Literal(literal));
        }
        Ok(Template { raw: raw.to_string(), segments })
    }

    pub fn has_placeholder(&self) -> bool {
        self.segments.iter().any(|s| matches!(s, Segment::Placeholder(_)))
    }

    /// Single-slot rule: an argv element that carries a placeholder may
    /// not also carry literal whitespace or shell metacharacters.
    pub fn violates_single_slot(&self) -> bool {
        const META: &[char] =
            &['"', '\'', '`', '$', '\\', '|', '&', ';', '<', '>', '(', ')', '*', '?', '~', '#'];
        if !self.has_placeholder() {
            return false;
        }
        self.segments.iter().any(|s| match s {
            Segment::Literal(lit) => lit.chars().any(|c| c.is_whitespace() || META.contains(&c)),
            Segment::Placeholder(_) => false,
        })
    }

    /// Expand against `ctx`. `quote_at_seam` is true only at the `print`
    /// seam, where every placeholder value is POSIX-single-quoted before
    /// it reaches the wrapper's `eval`.
    pub fn expand(&self, ctx: &ExpandCtx, quote_at_seam: bool) -> Result<String, ExpandError> {
        let mut out = String::new();
        for segment in &self.segments {
            match segment {
                Segment::Literal(lit) => out.push_str(lit),
                Segment::Placeholder(ph) => {
                    let value = ctx.resolve(ph)?;
                    // Every placeholder is quoted at the print seam, not
                    // just the path family: the whole line is eval'd, so a
                    // filename, query, or env value bearing shell
                    // metacharacters would otherwise inject.
                    if quote_at_seam {
                        if value.contains('\0') || value.contains('\n') {
                            return Err(ExpandError::HazardousPath);
                        }
                        out.push_str(&posix_single_quote(&value));
                    } else {
                        out.push_str(&value);
                    }
                }
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpandError {
    /// `{ext}` on a directory, `{repo_root}` with no `.git` ancestor.
    UndefinedPlaceholder(String),
    /// `{env.X}` not defined in the action scope; never empty.
    UndefinedEnv(String),
    /// NUL/newline inside a quoted value at the print seam.
    HazardousPath,
    /// A filesystem error during `{repo_root}`/`{ext}` resolution.
    PathResolution(String),
}

impl std::fmt::Display for ExpandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExpandError::UndefinedPlaceholder(p) => write!(f, "undefined placeholder {{{p}}}"),
            ExpandError::UndefinedEnv(n) => write!(f, "undefined env {{env.{n}}}"),
            ExpandError::HazardousPath => write!(f, "hazardous path (NUL/newline) at print seam"),
            ExpandError::PathResolution(e) => write!(f, "path resolution failed: {e}"),
        }
    }
}

pub struct ExpandCtx<'a> {
    /// Canonical absolute path of the selected candidate.
    pub path: &'a Path,
    pub query: &'a str,
    pub home: &'a str,
    /// Bindings set by `env` steps in this action. Not the process
    /// environment: a missing binding is undefined, never inherited.
    pub env: &'a HashMap<String, String>,
}

impl ExpandCtx<'_> {
    fn resolve(&self, ph: &Placeholder) -> Result<String, ExpandError> {
        match ph {
            Placeholder::Path => Ok(self.path.display().to_string()),
            Placeholder::Name => Ok(self
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.path.display().to_string())),
            Placeholder::Parent => {
                Ok(self.path.parent().unwrap_or(self.path).display().to_string())
            }
            Placeholder::Dir => {
                // A vanished selection is not a directory either; its
                // parent is the nearest place that may still exist.
                let is_dir = std::fs::metadata(self.path).map(|m| m.is_dir()).unwrap_or(false);
                let dir = if is_dir { self.path } else { self.path.parent().unwrap_or(self.path) };
                Ok(dir.display().to_string())
            }
            Placeholder::Ext => {
                let meta = std::fs::metadata(self.path)
                    .map_err(|e| ExpandError::PathResolution(e.to_string()))?;
                if !meta.is_file() {
                    return Err(ExpandError::UndefinedPlaceholder("ext".into()));
                }
                Ok(self
                    .path
                    .extension()
                    .map(|e| e.to_string_lossy().into_owned())
                    .unwrap_or_default())
            }
            Placeholder::RepoRoot => repo_root(self.path)
                .ok_or_else(|| ExpandError::UndefinedPlaceholder("repo_root".into()))
                .map(|p| p.display().to_string()),
            Placeholder::Home => Ok(self.home.to_string()),
            Placeholder::Query => Ok(self.query.to_string()),
            Placeholder::Env(name) => {
                self.env.get(name).cloned().ok_or_else(|| ExpandError::UndefinedEnv(name.clone()))
            }
        }
    }
}

/// Nearest ancestor (the candidate itself included) holding a `.git`
/// entry, regular file or directory, so linked worktrees resolve too.
pub fn repo_root(start: &Path) -> Option<PathBuf> {
    let mut cursor = Some(start);
    while let Some(dir) = cursor {
        if std::fs::metadata(dir.join(".git")).is_ok() {
            return Some(dir.to_path_buf());
        }
        cursor = dir.parent();
    }
    None
}

/// `'…'` with an inner `'` written as `'\''`.
pub fn posix_single_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for c in value.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}
