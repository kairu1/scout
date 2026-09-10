//! What each colour means in the picker, chosen once per run: gold for a
//! one-shot picker, burgundy for a session, so the frame itself says
//! which mode you are in before you read the title.
//!
//! Owns: the mapping from the palette to roles. Refuses to know about:
//! layout, glyphs, widths. Exposes: `Theme`, `Theme::for_session`.

use ratatui::style::Color;

use crate::palette::{self, Rgb};

fn color(rgb: Rgb) -> Color {
    Color::Rgb(rgb.0, rgb.1, rgb.2)
}

/// The roles a colour plays. Every styled span in the picker names one
/// of these, never a colour directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// Titles, the cursor, the selected row's marker, action keys, the
    /// warning banner: the mode's own colour.
    pub accent: Color,
    /// Frame lines, section titles, hints.
    pub chrome: Color,
    /// The selected row's text.
    pub text: Color,
    /// Secondary text: the disambiguating context beside a name, counters.
    pub muted: Color,
    /// Characters the query matched.
    pub matched: Color,
    /// The recon marker on a row with an unaccepted finding.
    pub finding: Color,
}

impl Theme {
    pub fn for_session(session: bool) -> Theme {
        if session {
            Theme {
                accent: color(palette::BURGUNDY),
                chrome: color(palette::MOCHA),
                text: color(palette::IVORY),
                muted: color(palette::NUDE),
                matched: color(palette::SEA_GREEN),
                finding: color(palette::GOLD),
            }
        } else {
            Theme {
                accent: color(palette::GOLD),
                chrome: color(palette::MOCHA),
                text: color(palette::IVORY),
                muted: color(palette::NUDE),
                matched: color(palette::SEA_GREEN),
                finding: color(palette::BURGUNDY),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two modes differ in their accent and in the finding marker,
    /// and share everything else, so a session is recognisable at a
    /// glance without the picker looking like a different tool.
    #[test]
    fn a_session_is_burgundy_and_a_one_shot_picker_is_gold() {
        let one_shot = Theme::for_session(false);
        let session = Theme::for_session(true);
        assert_eq!(one_shot.accent, color(palette::GOLD));
        assert_eq!(session.accent, color(palette::BURGUNDY));
        assert_ne!(one_shot.finding, one_shot.accent, "a finding is never the accent colour");
        assert_ne!(session.finding, session.accent);
        assert_eq!(one_shot.chrome, session.chrome);
        assert_eq!(one_shot.matched, session.matched);
    }
}
