//! The overlay's own primitives that draw nothing of the band's charts (ADR-0056: Your own widgets): [`Printed`], text
//! in pieces of their own colours on lines it never breaks, as the gutters and the figures are, which the library's
//! `Text`, in its style's one colour, does not draw, and `Rich`, which the scrollback's rows are, wraps; [`Hint`], the
//! completion ahead of the console's line, drawn only where it fits on one; and [`Tint`], a box filled with its style's colour that the pointer passes
//! through, as the band's panel and the lines that part its sections are, where a style's background would block the
//! pointer from the program beneath; and [`Grip`], the line between the band and the console, which a drag moves; and
//! [`TabButton`], a session tab, and [`StatusIcon`], the shape before its title that marks the session's status. Each
//! is laid out by the library and draws into the box it was given; the charts' primitives stand beside their own
//! drawing (`chart`, `gpu`, `lines`, `meters`), and paint what it fills ([`paint`]).

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
use aethereal_math::{Rect2, Vec2};
use aethereal_render::text::{Align, Outline, Run, Wrap};
use aethereal_ui::{Arrange, Draws, Laid, Measuring, Offer, Quad, Sample, Stateless, Style, TextDraw, Widget};

/// Text in pieces of their own colours, on as many lines as it breaks itself into, in its style's font and size, set
/// at the top left of its box and cut to it.
#[derive(Debug, Clone, PartialEq)]
pub struct Printed {
    pub runs: Vec<Run>,
}

impl Widget for Printed {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    /// As wide and tall as its lines, unbroken: nothing before its font's metrics come.
    fn measure(&self, style: &Style, measuring: &mut Measuring<'_>, _: Offer) -> Vec2 {
        let size = style.font_size * measuring.scale();
        let measured = measuring.text(style.font, size, &self.runs, Wrap::None);

        measured.map_or(Vec2::ZERO, |measured| Vec2::new(measured.width, measured.height))
    }

    /// One line, as tall as its font's line at its size, which even a row printed with nothing has: nothing before its
    /// font's metrics come.
    fn least(&self, style: &Style, measuring: &mut Measuring<'_>) -> Vec2 {
        let size = style.font_size * measuring.scale();
        let line = measuring.line(style.font, size).unwrap_or(0.0);

        Vec2::new(0.0, line)
    }

    fn draw(&self, _: &Stateless, style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        draws.text(TextDraw {
            at: laid.content.min,
            clip: laid.clip.intersection(laid.content),
            font: style.font,
            size: style.font_size * laid.scale,
            wrap: Wrap::None,
            align: Align::Left,
            outline: Outline::None,
            runs: self.runs.clone(),
        });
    }
}

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
