//! The picker.
//!
//! Owns: the picker's state and event loop, the panel layout, and how
//! rows, the action pane, the footer and the help overlay are drawn.
//! Refuses to know about: executing actions (it returns a
//! `DispatchRequest` and the caller executes it after teardown) and the
//! database (it is handed candidates).
//! Exposes: `run`, `DispatchRequest`, and the `glyph`, `render`, `strip`
//! and `terminal` modules.
//!
//! Drawing happens on stderr; stdout belongs to `print` steps so that
//! `eval "$(scout)"` works. Every string that reaches the screen passes
//! `strip::clean`. Layout is measured in terminal columns. Banners are
//! render states, never popups. The visual grammar: one accent colour,
//! name-first rows with location shown only as far as it disambiguates,
//! matcher hits in the accent, and a kind marker separating repositories
//! from directories from files. Rank is carried by order, not by per-row
//! ornament.

pub mod glyph;
pub mod render;
pub mod strip;
pub mod terminal;

use std::io::Stderr;
use std::path::PathBuf;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Terminal;

use crate::actions::Applicability;
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
    /// Caret position in the query, counted in CHARACTERS. Bytes would
    /// split a multi-byte character and panic on the next slice; the
    /// index is converted to a byte offset only at the edit site.
    caret: usize,
    /// Filter text for the action pane.
    action_query: String,
    /// Result rows the last frame had room for. The selection is clamped
    /// to this: without it `down` walks the selection past the last drawn
    /// row and the highlight sticks to the bottom while the real
    /// selection moves invisibly — the cursor appears to vanish.
    capacity: usize,
    /// `?` help overlay.
    help: bool,
    /// One-shot banner shown when no config file was found;
    /// dismissed on the first keystroke.
    no_config_banner: bool,
    /// What is true of the selected path, computed once per selection so
    /// `when` clauses are checked in memory and never stat per frame.
    applicability: Option<Applicability>,
    /// Every marker file any action asks about, probed once per selection.
    marker_set: Vec<String>,
    /// A one-frame footer message (a chord that does not apply here),
    /// cleared on the next key.
    notice: Option<String>,
}

impl App<'_> {
    fn refresh(&mut self) {
        let now = crate::platform::time::unix_now();
        self.results = search(&mut self.matcher, self.candidates, &self.query, now, RESULT_LIMIT);
        if self.selected >= self.results.len() {
            self.selected = self.results.len().saturating_sub(1);
        }
        self.applicability = None;
    }

    /// The applicability of the selected row, computed on first use after
    /// the selection changed. A handful of stats, once per selection.
    fn applicability(&mut self) -> Option<&Applicability> {
        let path = self.selected_result()?.path.clone();
        if self.applicability.as_ref().is_none_or(|a| a.path != path) {
            self.applicability = Some(Applicability::for_path(
                std::path::Path::new(&path),
                &self.marker_set,
                std::collections::HashSet::new(),
            ));
        }
        self.applicability.as_ref()
    }

    /// Does `action` apply to the selected row? `true` when nothing is
    /// selected and the action has no clause, so the pane still lists it.
    fn action_applies(&mut self, index: usize) -> bool {
        let Some(when) = self.config.actions[index].when.as_ref() else { return true };
        let when = when.clone();
        match self.applicability() {
            Some(a) => when.applies(a),
            None => false,
        }
    }

    /// Move the selection and forget the cached applicability.
    fn select(&mut self, index: usize) {
        if index != self.selected {
            self.selected = index;
            self.applicability = None;
        }
    }

    /// The selection, but only if it is a row that was actually drawn.
    /// Every dispatch path goes through here, so a row the user cannot
    /// see cannot be acted on.
    fn selected_result(&self) -> Option<&Ranked> {
        if self.selected >= self.reachable() {
            return None;
        }
        self.results.get(self.selected)
    }

    /// Byte offset of the caret, for slicing. Derived rather than
    /// stored, so the two can never disagree.
    fn caret_byte(&self) -> usize {
        self.query.char_indices().nth(self.caret).map(|(i, _)| i).unwrap_or(self.query.len())
    }

    fn insert(&mut self, c: char) {
        let at = self.caret_byte();
        self.query.insert(at, c);
        self.caret += 1;
        self.retarget();
    }

    /// A changed query means a different result set. Keeping the row
    /// index would silently retarget the selection onto an unrelated
    /// project at the same offset — and `enter` runs a command against
    /// it. The top row is the only defensible selection after a filter
    /// changes.
    fn retarget(&mut self) {
        self.selected = 0;
        self.refresh();
    }

    /// Backspace: delete the character *before* the caret.
    fn delete_back(&mut self) {
        if self.caret == 0 {
            return;
        }
        self.caret -= 1;
        let at = self.caret_byte();
        self.query.remove(at);
        self.retarget();
    }

    /// Delete: remove the character *under* the caret.
    fn delete_forward(&mut self) {
        let at = self.caret_byte();
        if at < self.query.len() {
            self.query.remove(at);
            self.retarget();
        }
    }

    /// How many result rows are reachable: what fits, never more than
    /// what is drawn.
    ///
    /// No `.max(1)` floor. A floor keeps row 0 selectable when the pane
    /// draws nothing at all (a terminal too short for a single row),
    /// which reopens an old defect: `enter`
    /// dispatching against a row the user never saw. Verified at 14 and
    /// 24 rows and not below, which is how it survived.
    fn reachable(&self) -> usize {
        self.results.len().min(self.capacity)
    }

    /// Actions that apply to the selection and match the pane's filter, as
    /// indices into `config.actions`. Substring over name and description,
    /// because a user who remembers "git" should find `status`, `log` and
    /// `diff`.
    fn filtered_actions(&mut self) -> Vec<usize> {
        let needle = self.action_query.to_lowercase();
        let candidates: Vec<usize> = self
            .config
            .actions
            .iter()
            .enumerate()
            .filter(|(_, a)| {
                needle.is_empty()
                    || a.name.to_lowercase().contains(&needle)
                    || a.description.to_lowercase().contains(&needle)
            })
            .map(|(i, _)| i)
            .collect();
        candidates.into_iter().filter(|&i| self.action_applies(i)).collect()
    }

    /// Why `action` is not offered here, for the footer.
    fn not_applicable_notice(&self, chord: &str, index: usize) -> String {
        let action = &self.config.actions[index];
        let reason = action.when.as_ref().map(|w| w.describe()).unwrap_or_default();
        format!("{chord} {}: {reason}", action.name)
    }

    /// Index of the action Enter dispatches (user config first).
    fn enter_action_index(&self) -> Option<usize> {
        let enter = self.config.enter_action()?;
        self.config.actions.iter().position(|a| std::ptr::eq(a, enter))
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
        caret: 0,
        menu: None,
        action_query: String::new(),
        capacity: DISPLAY_CAP,
        help: false,
        no_config_banner: config.source.is_none(),
        applicability: None,
        marker_set: marker_set(config),
        notice: None,
    };
    app.refresh();

    let mut terminal = terminal::enter()?;
    let outcome = event_loop(&mut terminal, &mut app);

    // Teardown must run whatever the loop produced, including a failed
    // disable_raw_mode, which previously took the `?` and left the user
    // inside the alternate screen.
    terminal::restore();
    outcome
}

/// The union of every action's `marker` list, so one probe per selection
/// answers every clause.
fn marker_set(config: &Config) -> Vec<String> {
    let mut set: Vec<String> = Vec::new();
    for action in &config.actions {
        if let Some(when) = &action.when {
            for m in &when.marker {
                if !set.contains(m) {
                    set.push(m.clone());
                }
            }
        }
    }
    set
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
        // Repeat as well as Press: under the kitty protocol and Windows
        // conhost a held key reports Repeat, and dropping it killed
        // auto-repeat for arrows and backspace.
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            continue;
        }
        tracing::debug!(code = ?key.code, mods = ?key.modifiers, "key");
        app.no_config_banner = false;
        app.notice = None;

        if app.help {
            app.help = false;
            continue;
        }

        if let Some(menu_index) = app.menu {
            // Quit is quit, from every surface. The action pane's own
            // Char arm excludes CONTROL, so without this Ctrl-C fell to
            // `_ => {}` and the built-in quit below was unreachable —
            // and raw mode clears ISIG, so there was no SIGINT either.
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                return Ok(None);
            }
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

        // Actions claim their chord before any built-in handling. The
        // loader has already refused picker-owned chords
        // and duplicates, so a match here is unambiguous.
        if let Some((chord, index)) = chord_action(app, &key) {
            if !app.action_applies(index) {
                app.notice = Some(app.not_applicable_notice(&chord, index));
                continue;
            }
            let name = app.config.actions[index].name.clone();
            if let Some(request) = app.dispatch(&name) {
                return Ok(Some(request));
            }
            continue;
        }

        match key.code {
            KeyCode::Esc => return Ok(None),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(None),
            KeyCode::Enter => {
                if let Some(index) = app.enter_action_index() {
                    if !app.action_applies(index) {
                        app.notice = Some(app.not_applicable_notice("enter", index));
                        continue;
                    }
                    let action_name = app.config.actions[index].name.clone();
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
            // Left/Right move the caret; the result list is navigated
            // with Up/Down, so the two never contend.
            KeyCode::Left => app.caret = app.caret.saturating_sub(1),
            KeyCode::Right => app.caret = (app.caret + 1).min(app.query.chars().count()),
            KeyCode::Home => app.caret = 0,
            KeyCode::End => app.caret = app.query.chars().count(),
            KeyCode::Delete => app.delete_forward(),
            KeyCode::Up => {
                let up = app.selected.saturating_sub(1);
                app.select(up);
            }
            KeyCode::Down => {
                // Clamped to what is drawn, not to how many were ranked.
                // The list is short on purpose: if the answer
                // is not here, the move is another keystroke, not a scroll.
                let down = (app.selected + 1).min(app.reachable().saturating_sub(1));
                app.select(down);
            }
            KeyCode::Backspace => app.delete_back(),
            // `?` opens the cheatsheet rather than searching for "?" —
            // a query that begins with it is the discoverability case
            // this exists for, and `esc` closes it without losing state.
            KeyCode::Char('?') if app.query.is_empty() => app.help = true,
            // A bracketed paste arrives as a run of Char events, so
            // inserting at the caret handles pasting for free.
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => app.insert(c),
            _ => {}
        }
    }
}

/// Columns the row prefix always spends: selection marker, a space,
/// the kind marker, a space. Derived rather than written as a literal so
/// a change to either glyph cannot leave the layout arithmetic behind.
const ROW_PREFIX_COLS: usize = 4;
/// Columns between the name column and the context column.
const NAME_GAP_COLS: usize = 2;
/// Share of the usable width the name column may take before it is
/// truncated, so context always has somewhere to go.
const NAME_COLUMN_PERCENT: usize = 60;

/// Display cap. The pane shows what fits; this stops a very tall
/// terminal turning the picker back into a scrolling window.
const DISPLAY_CAP: usize = 16;

/// The panel, centred with a margin. Generous rather than minimal: a
/// small box adrift in a black screen reads as unfinished, and the
/// border is what makes the surrounding space look like margin instead
/// of void.
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
    body_rows(body)
}

/// Rows a results pane of this size can show. The single definition.
fn body_rows(area: Rect) -> usize {
    (area.height.saturating_sub(2) as usize).min(DISPLAY_CAP)
}

/// The chord this key press spells and the action bound to it, if any.
/// `alt-<letter>` and `ctrl-<letter>` only: the closed set.
fn chord_action(app: &App<'_>, key: &crossterm::event::KeyEvent) -> Option<(String, usize)> {
    let KeyCode::Char(c) = key.code else { return None };
    let prefix = if key.modifiers.contains(KeyModifiers::ALT) {
        "alt-"
    } else if key.modifiers.contains(KeyModifiers::CONTROL) {
        "ctrl-"
    } else {
        return None;
    };
    let wanted = format!("{prefix}{}", c.to_ascii_lowercase());
    app.config
        .actions
        .iter()
        .position(|a| a.keybinding.as_deref() == Some(wanted.as_str()))
        .map(|i| (wanted, i))
}

fn draw(frame: &mut ratatui::Frame, app: &mut App<'_>) {
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

    // Too short to draw a single result row. Say so at the panel level:
    // the message cannot live inside the results pane, because at these
    // sizes that pane's interior is itself zero rows high — which is how
    // the first version of this signal came to be invisible at exactly
    // the sizes it was written for.
    if body_rows(body) == 0 {
        let inner = Rect {
            x: outer.x + 2,
            y: outer.y + 1,
            width: outer.width.saturating_sub(4),
            height: outer.height.saturating_sub(2).min(2),
        };
        frame.render_widget(
            Paragraph::new(Span::styled(
                "terminal too short - make the window taller",
                Style::default().fg(ACCENT),
            )),
            inner,
        );
        return;
    }

    draw_search(frame, app, search);
    if let (Some(area), Some((text, style))) = (banner_area, banner) {
        frame.render_widget(
            Paragraph::new(Span::styled(strip::clean(&text), style)),
            // Inset both sides: `..area` keeps the full width and runs
            // the text straight through the right border.
            Rect { x: area.x + 1, width: area.width.saturating_sub(2), ..area },
        );
    }

    // The action pane is a column, not a popup: a popup
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
    frame
        .render_widget(Paragraph::new(Line::from(query_spans(app, row[0].width as usize))), row[0]);
    frame.render_widget(
        Paragraph::new(Span::styled(counter, Style::default().fg(CHROME)))
            .alignment(ratatui::layout::Alignment::Right),
        row[1],
    );
}

/// Terminal columns a string occupies.
fn str_columns(text: &str) -> usize {
    use unicode_width::UnicodeWidthChar;
    text.chars().map(|c| c.width().unwrap_or(0)).sum()
}

/// The longest suffix of `text` that fits in `columns`.
fn take_last_columns(text: &str, columns: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let mut used = 0usize;
    let mut take = 0usize;
    for c in text.chars().rev() {
        let w = c.width().unwrap_or(0);
        if used + w > columns {
            break;
        }
        used += w;
        take += 1;
    }
    let skip = text.chars().count() - take;
    text.chars().skip(skip).collect()
}

/// The longest prefix of `text` that fits in `columns`.
fn take_first_columns(text: &str, columns: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let mut used = 0usize;
    let mut out = String::new();
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > columns {
            break;
        }
        used += w;
        out.push(c);
    }
    out
}

/// The query, with the caret drawn where it actually is. Each half is
/// stripped separately: `strip::clean` can remove characters, so
/// cleaning the whole string and then slicing by the caret would put the
/// cursor in the wrong place after a paste containing a control byte.
fn query_spans<'a>(app: &App<'_>, width: usize) -> Vec<Span<'a>> {
    let at = app.caret_byte();
    let (before, after) = app.query.split_at(at);
    // Scroll horizontally so the caret is always on screen. Without
    // this, typing past the field width moved the caret off the right
    // edge and further keystrokes produced no visible change at all.
    //
    // Measured in COLUMNS, not characters. A char-counted
    // window is right for ASCII and wrong by a factor of two for CJK,
    // which is the failure this whole layer exists to prevent — and the
    // first version of this scroll fix made exactly that mistake.
    let room = width.saturating_sub(3).max(1); // prompt, space, caret
    let before = take_last_columns(before, room);
    let after = take_first_columns(after, room.saturating_sub(str_columns(&before)));
    let (before, after) = (before.as_str(), after.as_str());
    let text = Style::default().add_modifier(Modifier::BOLD);
    let mut spans = vec![
        Span::styled(
            format!("{} ", glyph::PROMPT),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(strip::clean(before), text),
    ];
    // The caret belongs to the search field; while the action pane has
    // focus the field shows its text without one.
    if app.menu.is_none() {
        spans.push(Span::styled(glyph::CURSOR, Style::default().fg(ACCENT)));
    }
    spans.push(Span::styled(strip::clean(after), text));
    spans
}

/// Rows the picker will actually draw. `room` is derived from the same
/// helper the event loop clamps against, so the drawn list and the
/// reachable list are one fact rather than two that happen to agree.
fn visible<'a>(app: &'a App<'_>, area: Rect) -> &'a [Ranked] {
    let room = body_rows(area);
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
        // The zero-room case is handled at the panel level in `draw`,
        // where there is somewhere to put the message.
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

    // The name column is bounded. Unbounded, a single long basename
    // clipped itself with no marker AND drove `context_room` to zero,
    // which removed the disambiguating context from every *other* row —
    // one long name silently disabled the feature for the whole list.
    let widest = rows.iter().map(|r| render::display_width(&r.name)).max().unwrap_or(0);
    let usable = (inner.width as usize).saturating_sub(ROW_PREFIX_COLS + NAME_GAP_COLS);
    let name_col = widest.min(usable * NAME_COLUMN_PERCENT / 100);
    let context_room = usable.saturating_sub(name_col);

    // One clamped value drives both the row marker and ratatui's
    // highlight; two independent expressions could disagree on a resize.
    let selected = app.selected.min(shown.len().saturating_sub(1));
    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            ListItem::new(result_line(row, &shown[i].path, i == selected, name_col, context_room))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(selected));
    frame.render_stateful_widget(List::new(items), inner, &mut state);
}

/// The action pane: a searchable column. Filterable
/// because a config with a dozen actions is the case this exists for,
/// and scrolling a popup to find `diff` is worse than typing `di`.
fn draw_action_pane(frame: &mut ratatui::Frame, app: &mut App<'_>, area: Rect) {
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
            // A binding nobody can see is a binding nobody uses.
            if let Some(binding) = a.keybinding.as_deref() {
                spans.push(Span::styled(format!("  {binding}"), Style::default().fg(ACCENT)));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(menu_index));
    frame.render_stateful_widget(List::new(items), rows[2], &mut state);
}

/// The cheatsheet. Before it existed, `tab` was
/// discoverable only by reading the README.
fn draw_help(frame: &mut ratatui::Frame) {
    const ROWS: [(&str, &str); 9] = [
        ("type", "filter - results rank by name, then match, then frecency"),
        ("left / right", "move the cursor inside the search text"),
        ("home / end", "jump to the start or end of the search text"),
        ("backspace", "delete before the cursor; del deletes under it"),
        ("up / down", "move the selection"),
        ("enter", "run the default action on the selection"),
        ("tab", "open the action pane - it filters, so type to narrow"),
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
                Span::styled(format!("{k:>12}"), key),
                Span::styled(format!("  {v}"), label),
            ])
        })
        .collect();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(CHROME))
        .title(Span::styled(" keys ", Style::default().fg(CHROME)));
    let area = centered(frame.area(), 76, ROWS.len() as u16 + 2);
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// The kind marker. A `.git` stat, and only for
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

    let mut name = row.name.clone();
    render::truncate_right(&mut name, name_col);
    push_cells(&mut spans, &name, name_style, name_style, match_style);

    if !row.context.is_empty() && context_room > 2 {
        let pad = name_col.saturating_sub(render::display_width(&name)) + NAME_GAP_COLS;
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
    // A notice replaces the hints for one frame: the user just pressed a
    // key that did nothing, and the reason matters more than the legend.
    if let Some(notice) = &app.notice {
        frame.render_widget(
            Paragraph::new(Span::styled(format!(" {}", strip::clean(notice)), key)),
            area,
        );
        return;
    }
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
    // `?` opens help only on an empty query — otherwise it is a literal
    // character in the search. Advertising it unconditionally promised a
    // key that inserts a `?` the moment you have typed anything.
    if app.query.is_empty() {
        spans.push(Span::styled(format!("  {}  ", glyph::SEPARATOR), label));
        spans.push(Span::styled("?", key));
        spans.push(Span::styled(" keys", label));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(width: u16, height: u16) -> Rect {
        Rect { x: 0, y: 0, width, height }
    }

    /// The layout arithmetic must survive every terminal size, including
    /// the degenerate ones. Underflow here would panic inside a draw.
    #[test]
    fn layout_survives_every_terminal_size() {
        for width in [0u16, 1, 2, 3, 4, 5, 8, 20, 40, 200] {
            for height in [0u16, 1, 2, 3, 4, 5, 6, 8, 24, 80] {
                for banner in [false, true] {
                    let area = screen(width, height);
                    let p = panel(area);
                    assert!(p.width <= width.max(1) || width == 0);
                    let (search, _, body, footer) = regions(p, banner);
                    // Nothing may extend past the panel.
                    for r in [search, body, footer] {
                        assert!(
                            r.x + r.width <= p.x + p.width.max(1) + 1,
                            "{width}x{height} banner={banner}: region escapes the panel"
                        );
                    }
                    let _ = results_capacity(area, banner);
                }
            }
        }
    }

    /// A pane too short to draw a single row must report zero capacity —
    /// the `.max(1)` floor that used to sit here made row 0 selectable
    /// when nothing was drawn, so `enter` acted on an unseen row.
    #[test]
    fn a_pane_with_no_room_has_no_capacity() {
        assert_eq!(body_rows(screen(80, 0)), 0);
        assert_eq!(body_rows(screen(80, 1)), 0);
        assert_eq!(body_rows(screen(80, 2)), 0);
        assert_eq!(body_rows(screen(80, 3)), 1);
        // And the cap holds however tall the terminal is.
        assert_eq!(body_rows(screen(80, 500)), DISPLAY_CAP);
    }

    /// The search field scrolls in COLUMNS. A char-counted window is
    /// right for ASCII and wrong by a factor of two for CJK, which is
    /// how the first version of this fix still lost the caret.
    #[test]
    fn the_search_window_is_measured_in_columns() {
        assert_eq!(str_columns("abcd"), 4);
        assert_eq!(str_columns("日本語"), 6);

        // Suffix: 4 columns of a wide string is two glyphs, not four.
        assert_eq!(take_last_columns("日本語日", 4), "語日");
        assert_eq!(take_last_columns("abcdef", 4), "cdef");
        // A budget that splits a wide glyph takes the narrower fit.
        assert_eq!(take_last_columns("日本語", 3), "語");

        // Prefix, same rules.
        assert_eq!(take_first_columns("日本語日", 4), "日本");
        assert_eq!(take_first_columns("abcdef", 4), "abcd");
        assert_eq!(take_first_columns("日本語", 1), "");
        assert_eq!(take_first_columns("", 10), "");
    }

    /// Whatever the query and however narrow the field, the rendered
    /// row must fit — that is what keeps the caret on screen.
    #[test]
    fn the_rendered_query_row_never_exceeds_the_field() {
        let config = Config::builtin_only();
        let candidates: Vec<CandidateRow> = Vec::new();
        let state = IndexState::Empty;
        for query in
            ["", "a", "abcdefghij", &"x".repeat(200), "日本語版プロジェクト", &"語".repeat(50)]
        {
            for width in [4usize, 8, 12, 20, 40, 100] {
                let mut app = App {
                    config: &config,
                    candidates: &candidates,
                    index_state: &state,
                    home: String::new(),
                    matcher: crate::search::matcher::NucleoMatcher::new(),
                    query: query.to_string(),
                    caret: query.chars().count(),
                    results: Vec::new(),
                    selected: 0,
                    menu: None,
                    action_query: String::new(),
                    capacity: 8,
                    help: false,
                    no_config_banner: false,
                    applicability: None,
                    marker_set: Vec::new(),
                    notice: None,
                };
                app.caret = query.chars().count();
                let spans = query_spans(&app, width);
                let rendered: usize = spans.iter().map(|s| str_columns(&s.content)).sum();
                assert!(
                    rendered <= width,
                    "query {:?} at width {width} rendered {rendered} columns",
                    query
                );
                // The caret must be present whenever the field has focus.
                let has_caret = spans.iter().any(|s| s.content.contains(glyph::CURSOR));
                assert!(has_caret, "caret lost: query {:?} at width {width}", query);
            }
        }
    }
}
