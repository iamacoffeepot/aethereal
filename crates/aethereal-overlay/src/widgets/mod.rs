//! The overlay's own primitives that draw nothing of the band's charts (ADR-0056: Your own widgets): [`Hint`], the
//! completion ahead of the console's line, drawn only where it fits on one; [`Tint`], a box filled with its style's
//! colour that the pointer passes through, as the band's panel and the lines that part its sections are, where a
//! style's background would block the pointer from the program beneath; [`Grip`], the line between the band and the
//! console, which a drag moves; and [`TabButton`], a session tab, and [`StatusIcon`], the shape before its title that
//! marks the session's status. Each is laid out by the library and draws into the box it was given; the charts'
//! primitives stand beside their own drawing (`chart`, `gpu`, `lines`, `meters`), and paint what it fills
//! ([`paint`]). Text in pieces of their own colours is the library's: `Rich`, unbroken for the gutters, the figures,
//! and the tags, and `Selectable` for the rows the owner reads and copies; [`runs_of`] paints a piece's ink of the
//! palette as its run.

mod grip;
mod hint;
mod icon;
mod tab;

pub use grip::Grip;
pub use hint::Hint;
pub use icon::{Shape, StatusIcon};
pub use tab::TabButton;

use crate::chart::Fill;
use crate::colour::{Ink, Palette};
use crate::console::Piece;
use aethereal_math::Rect2;
use aethereal_render::text::Run;
use aethereal_ui::{Arrange, Draws, Laid, Quad, Sample, Stateless, Style, Widget};

/// Its box filled with its style's colour, over its style's background, the pointer passing through to what is
/// beneath where its style fills nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tint;

impl Widget for Tint {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    fn draw(&self, _: &Stateless, style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        paint(draws, [Fill { area: laid.rect, colour: style.colour }], laid.clip);
    }
}

/// Draws each of `fills` as a square-cornered quad of its colour, cut to `clip`.
pub fn paint(draws: &mut Draws<'_, '_>, fills: impl IntoIterator<Item = Fill>, clip: Rect2) {
    for Fill { area, colour } in fills {
        draws.quad(Quad {
            rect: area,
            clip,
            colour,
            border_width: 0.0,
            border_colour: colour,
            radius: 0.0,
            sample: Sample::Solid,
        });
    }
}

/// `pieces` as a text's runs, each piece in its ink of `palette`, opaque: a piece of no text left out, and a space for
/// none at all, as a text holds something.
pub fn runs_of(palette: &Palette, pieces: impl IntoIterator<Item = Piece>) -> Vec<Run> {
    let mut runs: Vec<Run> = pieces
        .into_iter()
        .filter(|piece| !piece.text.is_empty())
        .map(|piece| palette.run(&piece.text, piece.ink))
        .collect();
    if runs.is_empty() {
        runs.push(palette.run(" ", Ink::Text));
    }

    runs
}
