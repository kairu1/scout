//! The terminal's state: raw mode and the alternate screen, entered once
//! and always left, including on panic.

use std::io::Stderr;

use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

/// Enter raw mode and the alternate screen on stderr and hand back the
/// terminal to draw on. Stderr, because stdout belongs to the print seam.
pub fn enter() -> std::io::Result<Terminal<CrosstermBackend<Stderr>>> {
    install_panic_hook();
    enable_raw_mode()?;
    let mut stderr = std::io::stderr();
    // A child that ran on this terminal may have left attributes, mouse
    // reporting or bracketed paste behind. Reset what can be reset before
    // taking the screen, so the picker never inherits a child's state.
    let _ = crossterm::execute!(
        stderr,
        crossterm::style::ResetColor,
        crossterm::style::SetAttribute(crossterm::style::Attribute::Reset),
        crossterm::event::DisableMouseCapture,
        crossterm::event::DisableBracketedPaste,
        crossterm::terminal::EnableLineWrap
    );
    crossterm::execute!(stderr, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(std::io::stderr()))
}

/// Wait for one key with the terminal in raw mode but on the normal
/// screen, for the "press any key" pause after an in-process child.
/// Returns `true` when the key was Ctrl-C, which means leave scout.
pub fn wait_for_key() -> std::io::Result<bool> {
    enable_raw_mode()?;
    let ctrl_c = loop {
        match crossterm::event::read()? {
            crossterm::event::Event::Key(key)
                if matches!(
                    key.kind,
                    crossterm::event::KeyEventKind::Press | crossterm::event::KeyEventKind::Repeat
                ) =>
            {
                break key.code == crossterm::event::KeyCode::Char('c')
                    && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL);
            }
            _ => continue,
        }
    };
    disable_raw_mode()?;
    Ok(ctrl_c)
}

/// Best-effort return to the user's shell: leave raw mode, show the
/// cursor, leave the alternate screen. Every step is independent and
/// ignores its error, because this runs on paths where there is nothing
/// left to report the error to. Safe to call twice, and safe when setup
/// never got as far as raw mode.
pub fn restore() {
    let _ = disable_raw_mode();
    // ratatui hides the cursor on every frame and only shows it again in
    // `Terminal`'s Drop, which `panic = "abort"` never runs.
    let _ = crossterm::execute!(std::io::stderr(), crossterm::cursor::Show);
    let _ = crossterm::execute!(std::io::stderr(), LeaveAlternateScreen);
}

/// Restore the terminal before anything prints a panic. Without this, a
/// panic inside the picker leaves the terminal in raw mode inside the
/// alternate screen: the message is written to a screen that is
/// discarded, and the user lands in a shell that no longer echoes.
///
/// Works under `panic = "abort"`: the hook runs before the abort. The
/// panic is also logged, which is the only durable copy when the picker
/// owned stderr.
fn install_panic_hook() {
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            tracing::error!(panic = %info, "picker panicked");
            default_hook(info);
        }));
    });
}
