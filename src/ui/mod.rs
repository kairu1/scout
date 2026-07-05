//! TUI sector (3rd Rifles). ratatui + crossterm on STDERR — stdout is
//! reserved for the print seam so `eval "$(scout)"` works (ADR-003 §2).
//! Banners are first-class render states, never popups (ADR-001
//! §Degradation). Every rendered string passes the strip filter.

pub mod strip;

use std::io::Stderr;
use std::path::PathBuf;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Terminal;

use crate::config::Config;
use crate::search::matcher::NucleoMatcher;
use crate::search::{search, CandidateRow, IndexState, Ranked};

const RESULT_LIMIT: usize = 200;

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
    matcher: NucleoMatcher,
    query: String,
    results: Vec<Ranked>,
    selected: usize,
    /// Some(index into config.actions) = action menu open.
    menu: Option<usize>,
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
        matcher: NucleoMatcher::new(),
        query: String::new(),
        results: Vec::new(),
        selected: 0,
        menu: None,
        no_config_banner: config.source.is_none(),
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
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // query bar
            Constraint::Length(1), // banner
            Constraint::Min(1),    // results
            Constraint::Length(1), // footer
        ])
        .split(frame.size());

    let query_line = Line::from(vec![
        Span::styled("> ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(strip::clean(&app.query)),
    ]);
    frame.render_widget(Paragraph::new(query_line), chunks[0]);

    if let Some(banner) = banner_text(app) {
        frame.render_widget(
            Paragraph::new(Span::styled(
                strip::clean(&banner),
                Style::default().add_modifier(Modifier::REVERSED),
            )),
            chunks[1],
        );
    }

    let items: Vec<ListItem> = app
        .results
        .iter()
        .map(|r| ListItem::new(strip::clean(&r.path)))
        .collect();
    let mut list_state = ListState::default();
    if !app.results.is_empty() {
        list_state.select(Some(app.selected));
    }
    let list = List::new(items)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD))
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, chunks[2], &mut list_state);

    let enter_hint = app
        .config
        .enter_action()
        .map(|a| format!("enter:{}", a.name))
        .unwrap_or_else(|| "enter:-".into());
    let footer = format!(
        "{} candidate(s)  {}  tab:actions  esc:quit",
        app.results.len(),
        enter_hint
    );
    frame.render_widget(
        Paragraph::new(Span::styled(strip::clean(&footer), Style::default().add_modifier(Modifier::DIM))),
        chunks[3],
    );

    if let Some(menu_index) = app.menu {
        draw_action_menu(frame, app, menu_index);
    }
}

fn banner_text(app: &App<'_>) -> Option<String> {
    match app.index_state {
        IndexState::Empty => {
            Some("no paths indexed — run 'scout index <path>' to populate".into())
        }
        IndexState::FirstScanInProgress { rows_so_far } => Some(format!(
            "indexing in progress ({rows_so_far} paths so far) — results will appear when the first scan completes"
        )),
        IndexState::Ready { .. } => {
            if app.no_config_banner {
                Some(
                    "no config loaded — using built-in defaults; write \
                     $XDG_CONFIG_HOME/scout/config.toml to customise"
                        .into(),
                )
            } else {
                None
            }
        }
    }
}

fn draw_action_menu(frame: &mut ratatui::Frame, app: &App<'_>, menu_index: usize) {
    let area = centered(frame.size(), 50, (app.config.actions.len() as u16 + 2).min(12));
    let items: Vec<ListItem> = app
        .config
        .actions
        .iter()
        .map(|a| {
            let label = if a.description.is_empty() {
                a.name.clone()
            } else {
                format!("{} — {}", a.name, a.description)
            };
            ListItem::new(strip::clean(&label))
        })
        .collect();
    let mut state = ListState::default();
    state.select(Some(menu_index));
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("actions"))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD));
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
