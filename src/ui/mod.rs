//! TUI sector (3rd Rifles). ratatui + crossterm on STDERR — stdout is
//! reserved for the print seam so `eval "$(scout)"` works (ADR-003 §2).
//! Banners are first-class render states, never popups (ADR-001
//! §Degradation). Every rendered string passes the strip filter (the
//! path cells strip inline; chrome strings pass strip::clean).
//!
//! Visual grammar (see render.rs for the testable core, glyph.rs for
//! every character it emits): one amber accent, name-first rows with
//! location shown only as far as it disambiguates, matcher hits in the
//! accent, and a kind marker separating repositories from directories
//! from files (ADR-007). Rank is carried by order, not by per-row
//! ornament.

pub mod glyph;
pub mod render;
pub mod strip;

use std::io::Stderr;
use std::path::PathBuf;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Terminal;

use crate::config::Config;
use crate::search::matcher::NucleoMatcher;
use crate::search::{search, CandidateRow, IndexState, Ranked};

use render::{truncate_left, CellKind};

/// Ranking depth — how deep the matcher scores. Not what is shown.
const RESULT_LIMIT: usize = 200;
/// Display depth (ADR-007 §Decision 8a). If the answer is not in the
/// first few, the move is another keystroke, not a scroll.
const DISPLAY_LIMIT: usize = 8;

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
    /// `?` help overlay (ADR-007 §Decision 7).
    help: bool,
    /// One-shot banner shown when no config file was found (ADR-004 §7);
    /// dismissed on the first keystroke.
    no_config_banner: bool,
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
        help: false,
        no_config_banner: config.source.is_none(),
    };
    app.refresh();

    install_panic_hook();

    enable_raw_mode()?;
    let mut stderr = std::io::stderr();
    crossterm::execute!(stderr, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stderr()))?;

    let outcome = event_loop(&mut terminal, &mut app);

    // Teardown must run whatever the loop produced — including a failed
    // disable_raw_mode, which previously took the `?` and left the user
    // inside the alternate screen.
    restore_terminal();
    outcome
}

/// Best-effort return to the user's shell: leave raw mode, leave the
/// alternate screen. Every step is independent and ignores its error,
/// because this runs on paths where there is nothing left to report the
/// error to. Safe to call twice, and safe when setup never got as far as
/// putting the terminal into raw mode.
fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = crossterm::execute!(std::io::stderr(), LeaveAlternateScreen);
}

/// Restore the terminal before anything prints a panic.
///
/// Without this, a panic inside the picker leaves the terminal in raw
/// mode inside the alternate screen: the default hook writes the message
/// to a screen that is discarded on exit, and the user is dropped back
/// into a shell that no longer echoes. The message is lost and the
/// terminal needs `reset`.
///
/// This works under `panic = "abort"` (the release profile): the hook
/// runs before the abort. The panic is also written to the log file,
/// which is the only durable copy when the TUI owned stderr.
fn install_panic_hook() {
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore_terminal();
            tracing::error!(panic = %info, "picker panicked");
            default_hook(info);
        }));
    });
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stderr>>,
    app: &mut App<'_>,
) -> std::io::Result<Option<DispatchRequest>> {
    loop {
        terminal.draw(|frame| draw(frame, app))?;
        let Event::Key(key) = event::read()? else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        app.no_config_banner = false;

        if app.help {
            app.help = false;
            continue;
        }

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
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(None),
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
            // `?` opens the cheatsheet rather than searching for "?" —
            // a query that begins with it is the discoverability case
            // this exists for, and `esc` closes it without losing state.
            KeyCode::Char('?') if app.query.is_empty() => app.help = true,
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.query.push(c);
                app.refresh();
            }
            _ => {}
        }
    }
}

/// The surface is a card, not an application (ADR-007 §Decision 4): it
/// occupies the space it needs — query row, hairline, at most
/// DISPLAY_LIMIT results, a hint line — centred in the terminal, rather
/// than stretching to the corners with the list padded out by blanks.
fn card(area: Rect, app: &App<'_>, has_banner: bool) -> Rect {
    let rows = app.results.len().clamp(1, DISPLAY_LIMIT) as u16;
    let chrome = 2 + 1 + u16::from(has_banner); // query + hairline + hint
    let height = (rows + chrome).min(area.height);
    let width = area.width.min(96);
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 3,
        width,
        height,
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
        .split(card(frame.area(), app, banner.is_some()));

    draw_query_row(frame, app, chunks[0]);

    // Hairline separator: quiet structure instead of boxed chrome.
    frame.render_widget(
        Paragraph::new(Span::styled(
            glyph::HAIRLINE.repeat(chunks[1].width as usize),
            Style::default().fg(CHROME),
        )),
        chunks[1],
    );

    let mut next = 2;
    if let Some((text, style)) = banner {
        frame.render_widget(Paragraph::new(Span::styled(strip::clean(&text), style)), chunks[next]);
        next += 1;
    }

    // One column. The result list owns the full width (ADR-007): a
    // second panel is the thing a Spotlight-shaped surface does not have.
    draw_results(frame, app, chunks[next]);
    draw_footer(frame, app, chunks[next + 1]);

    if let Some(menu_index) = app.menu {
        draw_action_menu(frame, app, menu_index);
    }
    if app.help {
        draw_help(frame);
    }
}

/// The cheatsheet (ADR-007 §Decision 7). Until now `tab` was
/// discoverable only by reading the README.
fn draw_help(frame: &mut ratatui::Frame) {
    const ROWS: [(&str, &str); 6] = [
        ("type", "filter - results rank by match quality and frecency"),
        ("up / down", "move the selection"),
        ("enter", "run the default action on the selection"),
        ("tab", "open the action menu"),
        ("?", "this help"),
        ("esc", "close this, or quit"),
    ];
    let key = Style::default().fg(ACCENT).add_modifier(Modifier::BOLD);
    let label = Style::default().fg(CHROME);
    let lines: Vec<Line> = ROWS
        .iter()
        .map(|(k, v)| {
            Line::from(vec![
                Span::raw(" "),
                Span::styled(format!("{k:>10}"), key),
                Span::styled(format!("  {v}"), label),
            ])
        })
        .collect();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(CHROME))
        .title(Span::styled(" keys ", Style::default().fg(CHROME)));
    let area = centered(frame.area(), 72, ROWS.len() as u16 + 2);
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
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
        Span::styled(glyph::CURSOR, Style::default().fg(ACCENT)),
    ]);
    frame.render_widget(Paragraph::new(query_line), row[0]);
    frame.render_widget(
        Paragraph::new(Span::styled(counter, Style::default().fg(CHROME)))
            .alignment(ratatui::layout::Alignment::Right),
        row[1],
    );
}

/// Rows the picker will actually draw (ADR-007 §Decision 8a).
fn visible<'a>(app: &'a App<'_>, area: Rect) -> &'a [Ranked] {
    let room = (area.height as usize).min(DISPLAY_LIMIT);
    &app.results[..app.results.len().min(room)]
}

fn draw_results(frame: &mut ratatui::Frame, app: &App<'_>, area: Rect) {
    let shown = visible(app, area);
    if shown.is_empty() {
        return;
    }

    let paths: Vec<&str> = shown.iter().map(|r| r.path.as_str()).collect();
    let hits: Vec<&[u32]> = shown.iter().map(|r| r.match_indices.as_slice()).collect();
    let rows = render::rows(&paths, &hits, &app.home);

    // Name column is as wide as the widest name on screen, so contexts
    // line up — measured in columns (ADR-005), since the name is now the
    // aligned element.
    let name_col = rows.iter().map(|r| render::display_width(&r.name)).max().unwrap_or(0);
    // 2 marker + 2 kind + gap; leave the context whatever remains.
    let context_room = (area.width as usize).saturating_sub(name_col + 6);

    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            ListItem::new(result_line(
                row,
                &shown[i].path,
                i == app.selected,
                name_col,
                context_room,
            ))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.selected.min(shown.len().saturating_sub(1))));
    frame.render_stateful_widget(List::new(items), area, &mut state);
}

/// The kind marker (ADR-007 §Decision 8c). A `.git` stat, and only for
/// the handful of rows on screen — never in the indexer, which walks
/// 100k paths under a budget.
fn kind_marker(path: &str) -> char {
    let p = std::path::Path::new(path);
    match p.metadata() {
        Ok(meta) if meta.is_dir() => {
            if p.join(".git").exists() {
                glyph::KIND_REPO
            } else {
                glyph::KIND_DIR
            }
        }
        _ => glyph::KIND_FILE,
    }
}

fn result_line<'a>(
    row: &render::Row,
    path: &str,
    selected: bool,
    name_col: usize,
    context_room: usize,
) -> Line<'a> {
    let dim = if selected {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(CHROME)
    };
    let name_style =
        if selected { Style::default().add_modifier(Modifier::BOLD) } else { Style::default() };
    let match_style = Style::default().fg(ACCENT).add_modifier(Modifier::BOLD);

    let mut spans: Vec<Span<'a>> = Vec::with_capacity(8);
    spans.push(if selected {
        Span::styled(
            format!("{} ", glyph::SELECTED),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::raw(format!("{} ", glyph::UNSELECTED))
    });
    spans.push(Span::styled(format!("{} ", kind_marker(path)), Style::default().fg(CHROME)));

    push_cells(&mut spans, &row.name, name_style, name_style, match_style);

    if !row.context.is_empty() && context_room > 2 {
        let pad = name_col.saturating_sub(render::display_width(&row.name)) + 2;
        spans.push(Span::raw(" ".repeat(pad)));
        let mut context = row.context.clone();
        truncate_left(&mut context, context_room);
        push_cells(&mut spans, &context, dim, dim, match_style);
    }

    Line::from(spans)
}

/// Group consecutive same-kind cells into styled spans.
fn push_cells<'a>(
    spans: &mut Vec<Span<'a>>,
    cells: &[(char, CellKind)],
    dir: Style,
    base: Style,
    matched: Style,
) {
    let mut run = String::new();
    let mut run_kind: Option<CellKind> = None;
    for &(c, kind) in cells {
        if run_kind != Some(kind) {
            if let Some(prev) = run_kind {
                spans.push(Span::styled(
                    std::mem::take(&mut run),
                    style_for(prev, dir, base, matched),
                ));
            }
            run_kind = Some(kind);
        }
        run.push(c);
    }
    if let Some(prev) = run_kind {
        spans.push(Span::styled(run, style_for(prev, dir, base, matched)));
    }
}

fn style_for(kind: CellKind, dir: Style, base: Style, matched: Style) -> Style {
    match kind {
        CellKind::Dir => dir,
        CellKind::Base => base,
        CellKind::Match => matched,
    }
}

fn draw_footer(frame: &mut ratatui::Frame, app: &App<'_>, area: Rect) {
    let key = Style::default().fg(ACCENT);
    let label = Style::default().fg(CHROME);
    let mut spans = vec![Span::raw(" ")];
    if let Some(action) = app.config.enter_action() {
        spans.push(Span::styled("enter", key));
        spans.push(Span::styled(format!(" {}", strip::clean(&action.name)), label));
        spans.push(Span::styled(format!("  {}  ", glyph::SEPARATOR), label));
    }
    spans.push(Span::styled("tab", key));
    spans.push(Span::styled(" actions", label));
    spans.push(Span::styled(format!("  {}  ", glyph::SEPARATOR), label));
    spans.push(Span::styled("esc", key));
    spans.push(Span::styled(" quit", label));
    spans.push(Span::styled(format!("  {}  ", glyph::SEPARATOR), label));
    spans.push(Span::styled("?", key));
    spans.push(Span::styled(" keys", label));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn banner_text(app: &App<'_>) -> Option<(String, Style)> {
    let warn = Style::default().fg(ACCENT).add_modifier(Modifier::BOLD);
    let info = Style::default().fg(CHROME);
    match app.index_state {
        IndexState::Empty => Some((
            "no paths indexed - run 'scout index <path>' to populate".into(),
            warn,
        )),
        IndexState::FirstScanInProgress { rows_so_far } => Some((
            format!(
                "indexing in progress ({rows_so_far} paths so far) - results will appear when the first scan completes"
            ),
            warn,
        )),
        IndexState::Ready { .. } => {
            if app.no_config_banner {
                Some((
                    "no config loaded - using built-in defaults; write \
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
    let area = centered(frame.area(), width, height);

    let items: Vec<ListItem> = app
        .config
        .actions
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let selected = i == menu_index;
            let mut spans = vec![if selected {
                Span::styled(
                    format!("{} ", glyph::SELECTED),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                )
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
                spans.push(Span::styled(
                    "  enter",
                    Style::default().fg(ACCENT).add_modifier(Modifier::DIM),
                ));
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
            .title(Span::styled(
                " actions ",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
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
