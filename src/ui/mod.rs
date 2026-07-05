//! TUI sector (3rd Rifles). ratatui + crossterm on STDERR — stdout is
//! reserved for the print seam so `eval "$(scout)"` works (ADR-003 §2).
//! Banners are first-class render states, never popups (ADR-001
//! §Degradation). Every rendered string passes the strip filter (the
//! path cells strip inline; chrome strings pass strip::clean).
//!
//! Visual grammar (see render.rs for the testable core): one amber
//! accent, dim directory / bold basename path typography, matcher hits
//! in the accent, and a per-row frecency signal meter — ranking made
//! visible, not decorated.

pub mod preview;
pub mod render;
pub mod strip;

use std::io::Stderr;
use std::path::PathBuf;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Terminal;

use crate::config::Config;
use crate::search::matcher::NucleoMatcher;
use crate::search::{search, CandidateRow, IndexState, Ranked};

use render::{path_cells, signal_level, truncate_left, CellKind, SIGNAL_GLYPHS};

const RESULT_LIMIT: usize = 200;

const ACCENT: Color = Color::Yellow;
const CHROME: Color = Color::DarkGray;

/// What the user chose; the caller executes it after terminal teardown.
#[derive(Debug)]
pub struct DispatchRequest {
    pub action_name: String,
    pub candidate_id: i64,
    pub path: PathBuf,
    pub query: String,
}

struct App<'a> {
    config: &'a Config,
    candidates: &'a [CandidateRow],
    index_state: &'a IndexState,
    home: String,
    matcher: NucleoMatcher,
    query: String,
    results: Vec<Ranked>,
    selected: usize,
    /// Some(index into config.actions) = action menu open.
    menu: Option<usize>,
    /// One-shot banner shown when no config file was found (ADR-004 §7);
    /// dismissed on the first keystroke.
    no_config_banner: bool,
    /// Preview cache for the current selection, keyed by candidate id.
    preview: Option<(i64, preview::Preview)>,
}

impl App<'_> {
    fn refresh(&mut self) {
        let now = crate::index::unix_now();
        self.results = search(&mut self.matcher, self.candidates, &self.query, now, RESULT_LIMIT);
        if self.selected >= self.results.len() {
            self.selected = self.results.len().saturating_sub(1);
        }
    }

    fn selected_result(&self) -> Option<&Ranked> {
        self.results.get(self.selected)
    }

    /// Build (or reuse) the preview for the current selection. Called
    /// once per event-loop turn, before drawing.
    fn ensure_preview(&mut self) {
        let Some(current) = self.results.get(self.selected) else {
            self.preview = None;
            return;
        };
        if self.preview.as_ref().map(|(id, _)| *id) != Some(current.id) {
            self.preview =
                Some((current.id, preview::build(std::path::Path::new(&current.path))));
        }
    }

    fn dispatch(&self, action_name: &str) -> Option<DispatchRequest> {
        let result = self.selected_result()?;
        Some(DispatchRequest {
            action_name: action_name.to_string(),
            candidate_id: result.id,
            path: PathBuf::from(&result.path),
            query: self.query.clone(),
        })
    }
}

/// Run the picker. Returns the dispatch the user chose, or None on quit.
pub fn run(
    config: &Config,
    candidates: &[CandidateRow],
    index_state: &IndexState,
) -> std::io::Result<Option<DispatchRequest>> {
    let mut app = App {
        config,
        candidates,
        index_state,
        home: std::env::var("HOME").unwrap_or_default(),
        matcher: NucleoMatcher::new(),
        query: String::new(),
        results: Vec::new(),
        selected: 0,
        menu: None,
        no_config_banner: config.source.is_none(),
        preview: None,
    };
    app.refresh();

    enable_raw_mode()?;
    let mut stderr = std::io::stderr();
    crossterm::execute!(stderr, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stderr()))?;

    let outcome = event_loop(&mut terminal, &mut app);

    // Teardown must run whatever the loop produced.
    disable_raw_mode()?;
    crossterm::execute!(std::io::stderr(), LeaveAlternateScreen)?;
    outcome
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stderr>>,
    app: &mut App<'_>,
) -> std::io::Result<Option<DispatchRequest>> {
    loop {
        // Only pay the preview read when the pane will actually render;
        // on a narrow terminal the disk I/O is wasted (and needlessly
        // touches the selected candidate).
        let wide_enough = terminal.size().map(|s| s.width >= 70).unwrap_or(true);
        if wide_enough {
            app.ensure_preview();
        } else {
            app.preview = None;
        }
        terminal.draw(|frame| draw(frame, app))?;
        let Event::Key(key) = event::read()? else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        app.no_config_banner = false;

        if let Some(menu_index) = app.menu {
            match key.code {
                KeyCode::Esc | KeyCode::Tab => app.menu = None,
                KeyCode::Up => app.menu = Some(menu_index.saturating_sub(1)),
                KeyCode::Down => {
                    app.menu = Some((menu_index + 1).min(app.config.actions.len() - 1));
                }
                KeyCode::Enter => {
                    let action_name = app.config.actions[menu_index].name.clone();
                    if let Some(request) = app.dispatch(&action_name) {
                        return Ok(Some(request));
                    }
                    app.menu = None;
                }
                _ => {}
            }
            continue;
        }

        match key.code {
            KeyCode::Esc => return Ok(None),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(None)
            }
            KeyCode::Enter => {
                if let Some(action) = app.config.enter_action() {
                    let action_name = action.name.clone();
                    if let Some(request) = app.dispatch(&action_name) {
                        return Ok(Some(request));
                    }
                }
            }
            KeyCode::Tab => {
                if !app.config.actions.is_empty() && app.selected_result().is_some() {
                    app.menu = Some(0);
                }
            }
            KeyCode::Up => app.selected = app.selected.saturating_sub(1),
            KeyCode::Down => {
                if !app.results.is_empty() {
                    app.selected = (app.selected + 1).min(app.results.len() - 1);
                }
            }
            KeyCode::Backspace => {
                app.query.pop();
                app.refresh();
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.query.push(c);
                app.refresh();
            }
            _ => {}
        }
    }
}

fn draw(frame: &mut ratatui::Frame, app: &App<'_>) {
    let banner = banner_text(app);
    let mut constraints = vec![Constraint::Length(1), Constraint::Length(1)];
    if banner.is_some() {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Min(1));
    constraints.push(Constraint::Length(1));
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(frame.size());

    draw_query_row(frame, app, chunks[0]);

    // Hairline separator: quiet structure instead of boxed chrome.
    frame.render_widget(
        Paragraph::new(Span::styled(
            "\u{2500}".repeat(chunks[1].width as usize),
            Style::default().fg(CHROME),
        )),
        chunks[1],
    );

    let mut next = 2;
    if let Some((text, style)) = banner {
        frame.render_widget(
            Paragraph::new(Span::styled(strip::clean(&text), style)),
            chunks[next],
        );
        next += 1;
    }

    // Preview pane only when there is real width to spend on it.
    let body = chunks[next];
    if body.width >= 70 && app.preview.is_some() {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(body);
        draw_results(frame, app, cols[0]);
        draw_preview(frame, app, cols[1]);
    } else {
        draw_results(frame, app, body);
    }
    draw_footer(frame, app, chunks[next + 1]);

    if let Some(menu_index) = app.menu {
        draw_action_menu(frame, app, menu_index);
    }
}

fn draw_query_row(frame: &mut ratatui::Frame, app: &App<'_>, area: Rect) {
    let counter = format!("{}/{}", app.results.len(), app.candidates.len());
    let row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(counter.len() as u16 + 1)])
        .split(area);

    let query_line = Line::from(vec![
        Span::styled("\u{276f} ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        Span::raw(strip::clean(&app.query)),
        Span::styled("\u{2588}", Style::default().fg(ACCENT)),
    ]);
    frame.render_widget(Paragraph::new(query_line), row[0]);
    frame.render_widget(
        Paragraph::new(Span::styled(counter, Style::default().fg(CHROME)))
            .alignment(ratatui::layout::Alignment::Right),
        row[1],
    );
}

fn draw_results(frame: &mut ratatui::Frame, app: &App<'_>, area: Rect) {
    if app.results.is_empty() {
        if !app.query.is_empty() {
            frame.render_widget(
                Paragraph::new(Span::styled("  no matches", Style::default().fg(CHROME))),
                area,
            );
        }
        return;
    }

    // pointer(2) + path + gap(1) + meter(3) + gap(1) + visits(4)
    let meta_width = 9usize;
    let path_width = (area.width as usize).saturating_sub(2 + meta_width);

    let items: Vec<ListItem> = app
        .results
        .iter()
        .enumerate()
        .map(|(i, r)| ListItem::new(result_line(app, r, i == app.selected, path_width)))
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.selected));
    // Selection is styled inside result_line (rows light up from gray to
    // full); the List only supplies scroll-follow behaviour.
    frame.render_stateful_widget(List::new(items), area, &mut state);
}

fn result_line<'a>(app: &App<'_>, r: &Ranked, selected: bool, path_width: usize) -> Line<'a> {
    let mut cells = path_cells(&r.path, &app.home, &r.match_indices);
    truncate_left(&mut cells, path_width);

    let dir_style = if selected {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(CHROME)
    };
    let base_style = if selected {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let match_style = Style::default().fg(ACCENT).add_modifier(Modifier::BOLD);

    let mut spans: Vec<Span<'a>> = Vec::with_capacity(8);
    spans.push(if selected {
        Span::styled("\u{258c} ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
    } else {
        Span::raw("  ")
    });

    // Group consecutive same-kind cells into spans.
    let mut run = String::new();
    let mut run_kind: Option<CellKind> = None;
    let filled = cells.len();
    for (c, kind) in cells {
        if run_kind != Some(kind) {
            if let Some(prev) = run_kind {
                spans.push(Span::styled(
                    std::mem::take(&mut run),
                    style_for(prev, dir_style, base_style, match_style),
                ));
            }
            run_kind = Some(kind);
        }
        run.push(c);
    }
    if let Some(prev) = run_kind {
        spans.push(Span::styled(run, style_for(prev, dir_style, base_style, match_style)));
    }

    // Pad to the meta column, then the signal meter + visit count.
    spans.push(Span::raw(" ".repeat(path_width.saturating_sub(filled) + 1)));
    spans.push(Span::styled(
        SIGNAL_GLYPHS[signal_level(r.s_now)],
        Style::default().fg(ACCENT).add_modifier(Modifier::DIM),
    ));
    let visits = if r.visits_total > 0 { format!(" {:>4}", r.visits_total) } else { "     ".into() };
    spans.push(Span::styled(visits, Style::default().fg(CHROME)));

    Line::from(spans)
}

fn style_for(kind: CellKind, dir: Style, base: Style, matched: Style) -> Style {
    match kind {
        CellKind::Dir => dir,
        CellKind::Base => base,
        CellKind::Match => matched,
    }
}

fn draw_preview(frame: &mut ratatui::Frame, app: &App<'_>, area: Rect) {
    let Some((_, content)) = &app.preview else { return };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(CHROME))
        .title(Span::styled(
            format!(" {} ", strip::clean(&content.summary())),
            Style::default().fg(CHROME),
        ));
    let inner_height = area.height.saturating_sub(2) as usize;

    let lines: Vec<Line> = match content {
        preview::Preview::Dir { entries, truncated, .. } => {
            let mut lines: Vec<Line> = entries
                .iter()
                .take(inner_height)
                .map(|(name, is_dir)| {
                    if *is_dir {
                        Line::from(Span::styled(
                            format!("{}/", strip::clean(name)),
                            Style::default().add_modifier(Modifier::BOLD),
                        ))
                    } else {
                        Line::from(Span::raw(strip::clean(name)))
                    }
                })
                .collect();
            if *truncated || entries.len() > inner_height {
                lines.truncate(inner_height.saturating_sub(1));
                lines.push(Line::from(Span::styled("\u{2026}", Style::default().fg(CHROME))));
            }
            lines
        }
        preview::Preview::TextFile { lines: file_lines, truncated, .. } => {
            let mut lines: Vec<Line> = file_lines
                .iter()
                .take(inner_height)
                .map(|l| Line::from(Span::raw(strip::clean(l))))
                .collect();
            if *truncated || file_lines.len() > inner_height {
                lines.truncate(inner_height.saturating_sub(1));
                lines.push(Line::from(Span::styled("\u{2026}", Style::default().fg(CHROME))));
            }
            lines
        }
        preview::Preview::Binary { .. } => {
            vec![Line::from(Span::styled(
                "binary \u{2014} no preview",
                Style::default().fg(CHROME),
            ))]
        }
        preview::Preview::Special { kind } => {
            vec![Line::from(Span::styled(*kind, Style::default().fg(CHROME)))]
        }
        preview::Preview::Unreadable(err) => {
            vec![Line::from(Span::styled(
                format!("unreadable: {}", strip::clean(err)),
                Style::default().fg(CHROME),
            ))]
        }
    };

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_footer(frame: &mut ratatui::Frame, app: &App<'_>, area: Rect) {
    let key = Style::default().fg(ACCENT);
    let label = Style::default().fg(CHROME);
    let mut spans = vec![Span::raw(" ")];
    if let Some(action) = app.config.enter_action() {
        spans.push(Span::styled("enter", key));
        spans.push(Span::styled(format!(" {}", strip::clean(&action.name)), label));
        spans.push(Span::styled("  \u{00b7}  ", label));
    }
    spans.push(Span::styled("tab", key));
    spans.push(Span::styled(" actions", label));
    spans.push(Span::styled("  \u{00b7}  ", label));
    spans.push(Span::styled("esc", key));
    spans.push(Span::styled(" quit", label));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn banner_text(app: &App<'_>) -> Option<(String, Style)> {
    let warn = Style::default().fg(ACCENT).add_modifier(Modifier::BOLD);
    let info = Style::default().fg(CHROME);
    match app.index_state {
        IndexState::Empty => Some((
            "no paths indexed \u{2014} run 'scout index <path>' to populate".into(),
            warn,
        )),
        IndexState::FirstScanInProgress { rows_so_far } => Some((
            format!(
                "indexing in progress ({rows_so_far} paths so far) \u{2014} results will appear when the first scan completes"
            ),
            warn,
        )),
        IndexState::Ready { .. } => {
            if app.no_config_banner {
                Some((
                    "no config loaded \u{2014} using built-in defaults; write \
                     $XDG_CONFIG_HOME/scout/config.toml to customise"
                        .into(),
                    info,
                ))
            } else {
                None
            }
        }
    }
}

fn draw_action_menu(frame: &mut ratatui::Frame, app: &App<'_>, menu_index: usize) {
    let width = 56u16;
    let height = (app.config.actions.len() as u16 + 2).min(12);
    let area = centered(frame.size(), width, height);

    let items: Vec<ListItem> = app
        .config
        .actions
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let selected = i == menu_index;
            let mut spans = vec![if selected {
                Span::styled("\u{258c} ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            } else {
                Span::raw("  ")
            }];
            spans.push(Span::styled(
                strip::clean(&a.name),
                if selected {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            ));
            if a.keybinding.as_deref() == Some("enter") {
                spans.push(Span::styled("  enter", Style::default().fg(ACCENT).add_modifier(Modifier::DIM)));
            }
            if !a.description.is_empty() {
                spans.push(Span::styled(
                    format!("  {}", strip::clean(&a.description)),
                    Style::default().fg(CHROME),
                ));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(menu_index));
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(CHROME))
            .title(Span::styled(" actions ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))),
    );
    frame.render_widget(Clear, area);
    frame.render_stateful_widget(list, area, &mut state);
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}
