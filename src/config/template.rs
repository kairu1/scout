//! Template grammar (ADR-004 §4): closed eight-placeholder set,
//! `{{`/`}}` escapes, unknown placeholder = parse error at load, and the
//! single-slot rule for argv elements. Expansion implements the two
//! shell seams exactly (ADR-003 §2): POSIX-single-quoting of
//! path-valued placeholders happens only in `print` format strings.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placeholder {
    Path,
    Name,
    Parent,
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
            "ext" => Ok(Placeholder::Ext),
            "repo_root" => Ok(Placeholder::RepoRoot),
            "home" => Ok(Placeholder::Home),
            "query" => Ok(Placeholder::Query),
            _ => {
                if let Some(env_name) = name.strip_prefix("env.") {
                    let valid = !env_name.is_empty()
                        && env_name.len() <= 64
                        && env_name
                            .bytes()
                            .next()
                            .map(|b| b.is_ascii_alphabetic() || b == b'_')
                            .unwrap_or(false)
                        && env_name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
                    if valid {
                        return Ok(Placeholder::Env(env_name.to_string()));
                    }
                    return Err(format!("malformed env placeholder name `{env_name}`"));
                }
                Err(format!("unknown placeholder `{{{name}}}`"))
            }
        }
    }

    /// The three placeholders POSIX-single-quoted at the `print` seam.
    fn is_path_valued(&self) -> bool {
        matches!(self, Placeholder::Path | Placeholder::Parent | Placeholder::Home)
    }
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
    /// Pre-parse at config load (ADR-004 §10 step 7). Unknown
    /// placeholders and malformed braces refuse the file.
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

    /// Single-slot rule (ADR-004 §4): an argv element that carries a
    /// placeholder may not also carry literal whitespace or shell
    /// metacharacters.
    pub fn violates_single_slot(&self) -> bool {
        const META: &[char] = &[
            '"', '\'', '`', '$', '\\', '|', '&', ';', '<', '>', '(', ')', '*', '?', '~', '#',
        ];
        if !self.has_placeholder() {
            return false;
        }
        self.segments.iter().any(|s| match s {
            Segment::Literal(lit) => {
                lit.chars().any(|c| c.is_whitespace() || META.contains(&c))
            }
            Segment::Placeholder(_) => false,
        })
    }

    /// Expand against `ctx`. `quote_path_values` is true only at the
    /// `print` seam. Errors carry the ADR failure kind for tracing.
    pub fn expand(&self, ctx: &ExpandCtx, quote_path_values: bool) -> Result<String, ExpandError> {
        let mut out = String::new();
        for segment in &self.segments {
            match segment {
                Segment::Literal(lit) => out.push_str(lit),
                Segment::Placeholder(ph) => {
                    let value = ctx.resolve(ph)?;
                    if quote_path_values && ph.is_path_valued() {
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
    /// `{env.X}` not defined in the action scope (never empty — ADR-003 §5).
    UndefinedEnv(String),
    /// NUL/newline inside a quoted path at the print seam.
    HazardousPath,
    /// fs error during `{repo_root}`/`{ext}` resolution.
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
    /// Action-scope env (seeded sanitised, overlaid by env steps).
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
            Placeholder::Parent => Ok(self
                .path
                .parent()
                .unwrap_or(self.path)
                .display()
                .to_string()),
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
            Placeholder::Env(name) => self
                .env
                .get(name)
                .cloned()
                .ok_or_else(|| ExpandError::UndefinedEnv(name.clone())),
        }
    }
}

/// Nearest ancestor (the candidate itself included) holding a `.git`
/// entry — regular file or directory, so linked worktrees resolve
/// (ADR-004 §4). No other VCS marker in v1.
fn repo_root(start: &Path) -> Option<PathBuf> {
    let mut cursor = Some(start);
    while let Some(dir) = cursor {
        if std::fs::metadata(dir.join(".git")).is_ok() {
            return Some(dir.to_path_buf());
        }
        cursor = dir.parent();
    }
    None
}

/// `'…'` with inner `'` as `'\''` (ADR-003 §2 seam 1).
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
