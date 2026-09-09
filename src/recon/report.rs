//! The recon report: findings grouped by severity, in a human layout and
//! as TSV with the two unconstrained fields last.

use super::checks::Severity;
use super::run::WrapperSite;
use super::store::StoredFinding;

/// Findings on scout's own files (config, trust store, index, wrapper),
/// which are not index rows and are computed fresh on every run.
#[derive(Debug, Clone)]
pub struct OwnStateFinding {
    pub check: &'static str,
    pub severity: Severity,
    pub path: String,
    pub detail: String,
}

/// The `--since-last` view: findings first seen after the previous run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Since {
    /// When the previous run happened; `None` on the first run, when
    /// everything is new.
    pub previous: Option<i64>,
}

pub struct Report {
    pub findings: Vec<StoredFinding>,
    pub own_state: Vec<OwnStateFinding>,
    /// Where the shell wrapper was found.
    pub wrapper_sites: Vec<WrapperSite>,
    /// Show accepted findings too.
    pub show_accepted: bool,
    /// Set when the findings were narrowed to what is new.
    pub since: Option<Since>,
}

impl Report {
    /// Highest unaccepted severity, or `None` when the ground is clean.
    pub fn worst(&self) -> Option<Severity> {
        self.findings
            .iter()
            .filter(|f| f.accepted.is_none())
            .map(|f| f.severity)
            .chain(self.own_state.iter().map(|f| f.severity))
            .max()
    }

    /// 0 when nothing at or above `threshold` is unaccepted, else 1.
    pub fn exit_code(&self, threshold: Severity) -> u8 {
        match self.worst() {
            Some(worst) if worst >= threshold => 1,
            _ => 0,
        }
    }

    /// Human layout, grouped by severity, critical first; the summary
    /// line says how many accepted findings are hidden.
    pub fn render(&self, home: Option<&str>) -> String {
        let show = |s: &str| crate::ui::strip::clean(s);
        let tilde = |p: &str| match home {
            Some(h) if p == h => "~".to_string(),
            Some(h) if p.starts_with(&format!("{h}/")) => format!("~{}", &p[h.len()..]),
            _ => p.to_string(),
        };
        let mut out = String::new();
        if let Some(since) = self.since {
            let new = self.findings.iter().filter(|f| f.accepted.is_none()).count();
            out.push_str(&match since.previous {
                Some(t) => format!("{new} new since {}\n", iso_date(t)),
                None => format!("{new} new (first run)\n"),
            });
        }
        let mut counts = [0usize; 4];
        let mut hidden = 0usize;
        for severity in [Severity::Critical, Severity::High, Severity::Low] {
            let mut section = String::new();
            for f in &self.own_state {
                if f.severity == severity {
                    section.push_str(&format!(
                        "  {:<21} {}  {}\n",
                        f.check,
                        show(&tilde(&f.path)),
                        show(&f.detail)
                    ));
                }
            }
            for f in &self.findings {
                if f.severity != severity {
                    continue;
                }
                if let Some(reason) = &f.accepted {
                    if !self.show_accepted {
                        hidden += 1;
                        continue;
                    }
                    section.push_str(&format!(
                        "  {:<21} {}  {} (accepted: {})\n",
                        f.check.name(),
                        show(&tilde(&f.path)),
                        show(&f.detail),
                        show(reason)
                    ));
                    continue;
                }
                counts[severity as usize] += 1;
                section.push_str(&format!(
                    "  {:<21} {}  {}\n",
                    f.check.name(),
                    show(&tilde(&f.path)),
                    show(&f.detail)
                ));
            }
            if !section.is_empty() {
                out.push_str(&format!("\n{}\n", severity.as_str()));
                out.push_str(&section);
            }
        }
        let own = self.own_state.len();
        out.push_str(&format!(
            "\n{} critical, {} high, {} low{}{}\n",
            counts[Severity::Critical as usize]
                + self.own_state.iter().filter(|f| f.severity == Severity::Critical).count(),
            counts[Severity::High as usize]
                + self.own_state.iter().filter(|f| f.severity == Severity::High).count(),
            counts[Severity::Low as usize]
                + self.own_state.iter().filter(|f| f.severity == Severity::Low).count(),
            if hidden > 0 {
                format!(" ({hidden} accepted, hidden; --all shows them)")
            } else {
                String::new()
            },
            if own > 0 { format!("; {own} on scout's own files") } else { String::new() },
        ));
        if self.findings.is_empty() && self.own_state.is_empty() {
            out = match self.since {
                Some(Since { previous: Some(t) }) => format!("nothing new since {}\n", iso_date(t)),
                Some(Since { previous: None }) => "no findings (first run)\n".to_string(),
                None => "no findings\n".to_string(),
            };
        }
        for site in &self.wrapper_sites {
            out.push_str(&match site.line {
                Some(line) => format!(
                    "wrapper: {} line {line}\n",
                    show(&tilde(&site.path.display().to_string()))
                ),
                None => format!("wrapper: {}\n", show(&tilde(&site.path.display().to_string()))),
            });
        }
        out
    }

    /// `severity check first_seen last_seen accepted path detail`, one per
    /// line. `detail` is last because scout builds it from a path; `path`
    /// is second to last because it is the other unconstrained field and
    /// `detail` never contains a tab. Own-state rows carry `0 0` for the
    /// two timestamps and `-` for accepted.
    pub fn render_tsv(&self) -> String {
        let fold = |s: &str| s.replace(['\t', '\n'], " ");
        let mut out = String::new();
        for f in &self.own_state {
            out.push_str(&format!(
                "{}\t{}\t0\t0\t-\t{}\t{}\n",
                f.severity.as_str(),
                f.check,
                fold(&f.path),
                fold(&f.detail)
            ));
        }
        for f in &self.findings {
            if f.accepted.is_some() && !self.show_accepted {
                continue;
            }
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                f.severity.as_str(),
                f.check.name(),
                f.first_seen,
                f.last_seen,
                if f.accepted.is_some() { "accepted" } else { "-" },
                fold(&f.path),
                fold(&f.detail)
            ));
        }
        out
    }
}

/// A unix timestamp as an ISO-8601 date-time, for the human report.
fn iso_date(t: i64) -> String {
    let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(t.max(0) as u64);
    crate::platform::time::iso8601(at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recon::checks::Check;

    fn stored(
        check: Check,
        severity: Severity,
        path: &str,
        accepted: Option<&str>,
    ) -> StoredFinding {
        StoredFinding {
            path_id: 1,
            path: path.to_string(),
            check,
            severity,
            detail: "d".into(),
            fact: "f".into(),
            first_seen: 1,
            last_seen: 2,
            accepted: accepted.map(str::to_string),
        }
    }

    #[test]
    fn exit_code_keys_off_the_threshold_and_ignores_accepted() {
        let report = Report {
            findings: vec![
                stored(Check::GroupWritable, Severity::Low, "/a", None),
                stored(Check::Suid, Severity::Critical, "/b", Some("known")),
            ],
            own_state: vec![],
            wrapper_sites: vec![],
            show_accepted: false,
            since: None,
        };
        assert_eq!(report.worst(), Some(Severity::Low));
        assert_eq!(report.exit_code(Severity::High), 0, "an accepted critical does not fail");
        assert_eq!(report.exit_code(Severity::Low), 1);
        let clean = Report {
            findings: vec![],
            own_state: vec![],
            wrapper_sites: vec![],
            show_accepted: false,
            since: None,
        };
        assert_eq!(clean.exit_code(Severity::Low), 0);
        assert!(clean.render(None).contains("no findings"));
    }

    /// `--since-last` heads the report with how many are new and since
    /// when; a clean view says so; the first run says it is the first.
    #[test]
    fn since_last_heads_the_report_with_the_count_and_the_previous_run() {
        let report = Report {
            findings: vec![stored(Check::Suid, Severity::Critical, "/b", None)],
            own_state: vec![],
            wrapper_sites: vec![],
            show_accepted: false,
            since: Some(Since { previous: Some(1_700_000_000) }),
        };
        let text = report.render(None);
        assert!(text.starts_with("1 new since 2023-11-14T22:13:20Z\n"), "{text}");
        let clean = Report { findings: vec![], ..report };
        assert!(clean.render(None).starts_with("nothing new since 2023"), "{}", clean.render(None));
        let first = Report { since: Some(Since { previous: None }), ..clean };
        assert!(first.render(None).starts_with("no findings (first run)"));
    }

    /// The wrapper's location closes the human report and never enters
    /// the TSV, which lists findings only.
    #[test]
    fn the_wrapper_site_is_reported_with_its_line_and_kept_out_of_tsv() {
        let report = Report {
            findings: vec![],
            own_state: vec![],
            wrapper_sites: vec![WrapperSite { path: "/home/u/.bashrc".into(), line: Some(42) }],
            show_accepted: false,
            since: None,
        };
        assert!(report.render(Some("/home/u")).ends_with("wrapper: ~/.bashrc line 42\n"));
        assert_eq!(report.render_tsv(), "");
    }

    #[test]
    fn human_render_groups_by_severity_and_hides_accepted_unless_asked() {
        let mut report = Report {
            findings: vec![
                stored(Check::GroupWritable, Severity::Low, "/home/u/a", None),
                stored(Check::WorldWritableDir, Severity::Critical, "/home/u/b", None),
                stored(Check::Suid, Severity::Critical, "/home/u/c", Some("ok")),
            ],
            own_state: vec![],
            wrapper_sites: vec![],
            show_accepted: false,
            since: None,
        };
        let text = report.render(Some("/home/u"));
        let crit = text.find("\ncritical\n").unwrap();
        let low = text.find("\nlow\n").unwrap();
        assert!(crit < low, "critical before low:\n{text}");
        assert!(text.contains("~/b"), "home collapsed:\n{text}");
        assert!(!text.contains("~/c"), "accepted hidden by default:\n{text}");
        assert!(text.contains("1 accepted, hidden"), "{text}");
        report.show_accepted = true;
        assert!(report.render(Some("/home/u")).contains("(accepted: ok)"));
    }

    #[test]
    fn tsv_puts_path_then_detail_last_and_folds_tabs() {
        let mut f = stored(Check::WorldWritableFile, Severity::High, "/x/we\tird", None);
        f.detail = "line\nbreak".into();
        let report = Report {
            findings: vec![f],
            own_state: vec![],
            wrapper_sites: vec![],
            show_accepted: false,
            since: None,
        };
        let line = report.render_tsv();
        let fields: Vec<&str> = line.trim_end().splitn(7, '\t').collect();
        assert_eq!(fields[0], "high");
        assert_eq!(fields[1], "world-writable-file");
        assert_eq!(fields[4], "-");
        assert_eq!(fields[5], "/x/we ird", "a tab in the path is folded");
        assert_eq!(fields[6], "line break");
    }
}
