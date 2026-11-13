//! The overlay's own primitives that draw nothing of the band's charts (ADR-0056: Your own widgets): [`Printed`], text
//! in pieces of their own colours, as the gutters, the figures, and the scrollback's rows are, which the library's
//! `Text`, in its style's one colour, does not draw; and [`Tint`], a box filled with its style's colour that the pointer passes
//! through, as the band's panel and the lines that part its sections are, where a style's background would block the
//! pointer from the program beneath. Each is laid out by the library and draws into the box it was given; the charts'
//! primitives stand beside their own drawing (`chart`, `gpu`, `lines`, `meters`), and paint what it fills ([`paint`]).

use crate::chart::Fill;
use crate::colour::BONE;
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

/// `pieces` as a text's runs, each piece's colour opaque: a piece of no text left out, and a space for none at all, as
/// a text holds something.
pub fn runs_of(pieces: impl IntoIterator<Item = Piece>) -> Vec<Run> {
    let mut runs: Vec<Run> = pieces
        .into_iter()
        .filter(|piece| !piece.text.is_empty())
        .map(|piece| Run::new(&piece.text, piece.swatch.opaque()))
        .collect();
    if runs.is_empty() {
        runs.push(Run::new(" ", BONE.opaque()));
    }

    runs
}
