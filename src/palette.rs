//! The six colours scout paints with, as one source: the picker's theme
//! and the tmux options scout sets on its own server both derive from
//! these, so the frame in the terminal and the borders around it agree.
//!
//! Owns: the palette and its rendering as `#rrggbb`. Refuses to know
//! about: ratatui, tmux, which colour means what. Exposes: the six
//! constants, `Rgb`, `Rgb::hex`.

/// One colour, 8 bits per channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `#rrggbb`, the form tmux and most tools take.
    pub fn hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.0, self.1, self.2)
    }
}

pub const IVORY: Rgb = Rgb(0xED, 0xE9, 0xE3);
pub const NUDE: Rgb = Rgb(0xD2, 0xB5, 0x96);
pub const MOCHA: Rgb = Rgb(0xA4, 0x78, 0x64);
pub const BURGUNDY: Rgb = Rgb(0x67, 0x19, 0x2E);
pub const GOLD: Rgb = Rgb(0xC9, 0xA4, 0x4C);
pub const SEA_GREEN: Rgb = Rgb(0x3E, 0x6E, 0x58);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_render_as_upper_case_hex() {
        assert_eq!(BURGUNDY.hex(), "#67192E");
        assert_eq!(IVORY.hex(), "#EDE9E3");
        assert_eq!(SEA_GREEN.hex(), "#3E6E58");
    }
}
