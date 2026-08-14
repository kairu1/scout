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
    /// Some(index into the FILTERED action list) = action pane open.
    menu: Option<usize>,
    /// Filter text for the action pane (ADR-007 rev 2).
    action_query: String,
    /// Result rows the last frame had room for. The selection is clamped
    /// to this: without it `down` walks the selection past the last drawn
    /// row and the highlight sticks to the bottom while the real
    /// selection moves invisibly — the cursor appears to vanish.
    capacity: usize,
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

    /// How many result rows are reachable: what fits, never more than
    /// what is drawn.
    fn reachable(&self) -> usize {
        self.results.len().min(self.capacity.max(1))
    }

    /// Actions matching the pane's filter, as indices into
    /// `config.actions`. Substring over name and description, because a
    /// user who remembers "git" should find `status`, `log` and `diff`.
    fn filtered_actions(&self) -> Vec<usize> {
        let needle = self.action_query.to_lowercase();
        self.config
            .actions
            .iter()
            .enumerate()
            .filter(|(_, a)| {
                needle.is_empty()
                    || a.name.to_lowercase().contains(&needle)
                    || a.description.to_lowercase().contains(&needle)
            })
            .map(|(i, _)| i)
            .collect()
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
        action_query: String::new(),
        capacity: DISPLAY_CAP,
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
        // Capacity is computed from the same layout the frame will use,
        // so movement can never outrun what is drawn.
        let size = terminal.size()?;
        let screen = Rect { x: 0, y: 0, width: size.width, height: size.height };
        app.capacity = results_capacity(screen, banner_text(app).is_some());
        if app.selected >= app.reachable() {
            app.selected = app.reachable().saturating_sub(1);
        }
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
            let matches = app.filtered_actions();
            match key.code {
                KeyCode::Esc | KeyCode::Tab => {
                    app.menu = None;
                    app.action_query.clear();
                }
                KeyCode::Up => app.menu = Some(menu_index.saturating_sub(1)),
                KeyCode::Down => {
                    app.menu = Some((menu_index + 1).min(matches.len().saturating_sub(1)));
                }
                KeyCode::Enter => {
                    if let Some(&action) = matches.get(menu_index) {
                        let action_name = app.config.actions[action].name.clone();
                        if let Some(request) = app.dispatch(&action_name) {
                            return Ok(Some(request));
                        }
                    }
                    app.menu = None;
                    app.action_query.clear();
                }
                KeyCode::Backspace => {
                    app.action_query.pop();
                    app.menu = Some(0);
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.action_query.push(c);
                    app.menu = Some(0);
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
                // Clamped to what is drawn, not to how many were ranked.
                // ADR-007 §3 keeps the list short on purpose: if the answer
                // is not here, the move is another keystroke, not a scroll.
                app.selected = (app.selected + 1).min(app.reachable().saturating_sub(1));
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

/// Display cap. The pane shows what fits; this stops a very tall
/// terminal turning the picker back into a scrolling window (ADR-007
/// §Decision 3).
const DISPLAY_CAP: usize = 16;

/// The panel, centred with a margin. Generous rather than minimal: a
/// small box adrift in a black screen reads as unfinished, and the
/// border is what makes the surrounding space look like margin instead
/// of void (ADR-007 rev 2).
fn panel(area: Rect) -> Rect {
    let width = area.width.saturating_sub(6).clamp(20, 120);
    let height = area.height.saturating_sub(2).clamp(8, 30);
    centered(area, width, height)
}

/// Vertical regions inside the panel: search field, optional banner,
/// body, footer. One function so the event loop and the renderer cannot
/// disagree about how many rows the body has.
fn regions(area: Rect, has_banner: bool) -> (Rect, Option<Rect>, Rect, Rect) {
    let inner = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    let mut constraints = vec![Constraint::Length(3)];
    if has_banner {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Min(3));
    constraints.push(Constraint::Length(1));
    let rows =
        Layout::default().direction(Direction::Vertical).constraints(constraints).split(inner);
    let mut i = 1;
    let banner = if has_banner {
        i += 1;
        Some(rows[1])
    } else {
        None
    };
    (rows[0], banner, rows[i], rows[i + 1])
}

/// Result rows the body can actually show. The event loop clamps the
/// selection to this, so the cursor can never leave the drawn list.
fn results_capacity(area: Rect, has_banner: bool) -> usize {
    let (_, _, body, _) = regions(panel(area), has_banner);
    (body.height.saturating_sub(2) as usize).min(DISPLAY_CAP)
}

fn draw(frame: &mut ratatui::Frame, app: &App<'_>) {
    let banner = banner_text(app);
    let outer = panel(frame.area());

    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(CHROME))
            .title(Span::styled(
                " scout ",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
        outer,
    );

    let (search, banner_area, body, footer) = regions(outer, banner.is_some());
    draw_search(frame, app, search);
    if let (Some(area), Some((text, style))) = (banner_area, banner) {
        frame.render_widget(
            Paragraph::new(Span::styled(strip::clean(&text), style)),
            // Inset both sides: `..area` keeps the full width and runs
            // the text straight through the right border.
            Rect { x: area.x + 1, width: area.width.saturating_sub(2), ..area },
        );
    }

    // The action pane is a column, not a popup (ADR-007 rev 2): a popup
    // covers the results you are choosing an action FOR, and it cannot
    // hold a filter row without covering more of them.
    if app.menu.is_some() {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
            .split(body);
        draw_results(frame, app, cols[0]);
        draw_action_pane(frame, app, cols[1]);
    } else {
        draw_results(frame, app, body);
    }

    draw_footer(frame, app, footer);
    if app.help {
        draw_help(frame);
    }
}

/// The search field, drawn as a field. It was a bare line before, which
/// is why the surface read as a terminal that happened to echo rather
/// than as something you type into.
fn draw_search(frame: &mut ratatui::Frame, app: &App<'_>, area: Rect) {
    let counter = format!("{}/{} ", app.results.len(), app.candidates.len());
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(if app.menu.is_some() { CHROME } else { ACCENT }))
            .title(Span::styled(" search ", Style::default().fg(CHROME))),
        area,
    );
    let inner =
        Rect { x: area.x + 2, y: area.y + 1, width: area.width.saturating_sub(4), height: 1 };
    let row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(counter.len() as u16)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!("{} ", glyph::PROMPT),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(strip::clean(&app.query), Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                if app.menu.is_none() { glyph::CURSOR } else { "" },
                Style::default().fg(ACCENT),
            ),
        ])),
        row[0],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(counter, Style::default().fg(CHROME)))
            .alignment(ratatui::layout::Alignment::Right),
        row[1],
    );
}

/// Rows the picker will actually draw.
fn visible<'a>(app: &'a App<'_>, area: Rect) -> &'a [Ranked] {
    let room = (area.height.saturating_sub(2) as usize).min(DISPLAY_CAP);
    &app.results[..app.results.len().min(room)]
}

fn draw_results(frame: &mut ratatui::Frame, app: &App<'_>, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(CHROME))
        .title(Span::styled(" results ", Style::default().fg(CHROME)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let shown = visible(app, area);
    if shown.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "  no matches",
                Style::default().fg(CHROME).add_modifier(Modifier::DIM),
            )),
            inner,
        );
        return;
    }

    let paths: Vec<&str> = shown.iter().map(|r| r.path.as_str()).collect();
    let hits: Vec<&[u32]> = shown.iter().map(|r| r.match_indices.as_slice()).collect();
    let rows = render::rows(&paths, &hits, &app.home);

    let name_col = rows.iter().map(|r| render::display_width(&r.name)).max().unwrap_or(0);
    let context_room = (inner.width as usize).saturating_sub(name_col + 6);

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
    frame.render_stateful_widget(List::new(items), inner, &mut state);
}

/// The action pane: a searchable column (ADR-007 rev 2). Filterable
/// because a config with a dozen actions is the case this exists for,
/// and scrolling a popup to find `diff` is worse than typing `di`.
fn draw_action_pane(frame: &mut ratatui::Frame, app: &App<'_>, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(" actions ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)])
        .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!("{} ", glyph::PROMPT),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(strip::clean(&app.action_query), Style::default()),
            Span::styled(glyph::CURSOR, Style::default().fg(ACCENT)),
        ])),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            glyph::HAIRLINE.repeat(rows[1].width as usize),
            Style::default().fg(CHROME),
        )),
        rows[1],
    );

    let matches = app.filtered_actions();
    if matches.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled("  no action matches", Style::default().fg(CHROME))),
            rows[2],
        );
        return;
    }

    let menu_index = app.menu.unwrap_or(0).min(matches.len() - 1);
    let items: Vec<ListItem> = matches
        .iter()
        .enumerate()
        .map(|(i, &action)| {
            let a = &app.config.actions[action];
            let selected = i == menu_index;
            let mut spans = vec![if selected {
                Span::styled(
                    format!("{} ", glyph::SELECTED),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw(format!("{} ", glyph::UNSELECTED))
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
                spans.push(Span::styled("  enter", Style::default().fg(ACCENT)));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(menu_index));
    frame.render_stateful_widget(List::new(items), rows[2], &mut state);
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
