//! What a widget draws: quads, filled or sampled from a texture, and texts, in the order it draws them, a later one
//! over an earlier. The screen keeps each by the widget's key and its place among the widget's draws, and sends only
//! what changed (ADR-0056: Draws are kept by widget key).

use crate::fonts::Measuring;
use crate::style::Edges;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::text::{Align, Colour, Font, Outline, Run, Wrap};
use aethereal_render::texture::TextureId;

/// Where a widget's draws go, in order, with the fonts it measures its text by, and whether it has its screen's focus.
pub struct Draws<'draws, 'fonts> {
    pieces: &'draws mut Vec<Piece>,
    measuring: &'draws mut Measuring<'fonts>,
    focus: Focused,
}

/// Whether a widget being drawn has its screen's focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focused {
    Yes,
    No,
}

impl<'draws, 'fonts> Draws<'draws, 'fonts> {
    /// Draws into `pieces`, measuring with `measuring`, of a widget that has its screen's focus or not, as `focus` says.
    pub(crate) const fn new(
        pieces: &'draws mut Vec<Piece>,
        measuring: &'draws mut Measuring<'fonts>,
        focus: Focused,
    ) -> Self {
        Self { pieces, measuring, focus }
    }

    /// Whether the widget has its screen's focus, as a text input shows its caret only while it has.
    #[must_use]
    pub fn focused(&self) -> bool {
        self.focus == Focused::Yes
    }

    /// Draws `quad` over what was drawn before it.
    pub fn quad(&mut self, quad: Quad) {
        self.pieces.push(Piece::Quad(quad));
    }

    /// Draws `text` over what was drawn before it.
    pub fn text(&mut self, text: TextDraw) {
        self.pieces.push(Piece::Text(text));
    }

    /// The fonts the screen measures text by, as layout measured it: a widget that draws text lays it out as it
    /// measured it.
    pub const fn measuring(&mut self) -> &mut Measuring<'fonts> {
        self.measuring
    }
}

/// One thing a widget draws.
#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    Quad(Quad),
    Text(TextDraw),
}

/// A box, in its screen's pixels, filled with `colour` or with a texture tinted by it, rounded at its corners by
/// `radius` pixels, with a line `border_width` pixels wide inside its edge in `border_colour`, and cut to `clip`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    pub rect: Rect2,
    pub clip: Rect2,
    pub colour: Colour,
    pub border_width: f32,
    pub border_colour: Colour,
    pub radius: f32,
    pub sample: Sample,
}

/// What fills a quad.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sample {
    /// Its colour alone.
    Solid,
    /// The part `uv` of `texture`, from 0 to 1 across and down, its edges kept as `slices` says, tinted by its colour.
    Texture { texture: TextureId, uv: Rect2, slices: Slices },
}

/// How a sampled quad keeps the edges of what it samples: each side's band, `inset` pixels of the quad drawn from
/// `uv` of the texture's width or height, unstretched as the quad grows, its corners unscaled either way, and the middle
/// stretched over what is left. No bands stretch the whole.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slices {
    pub inset: Edges<f32>,
    pub uv: Edges<f32>,
}

impl Slices {
    /// No bands: the whole is stretched.
    pub const NONE: Self = Self { inset: Edges::all(0.0), uv: Edges::all(0.0) };
}

/// A text, laid out as the renderer lays out [`CreateText`](aethereal_render::text::CreateText): `runs` in `font` at
/// `size` pixels to the em, its lines broken as `wrap` says and set as `align` says, with a rim as `outline` says; its
/// first line's top left at `at`, in its screen's pixels, and cut to `clip`.
#[derive(Debug, Clone, PartialEq)]
pub struct TextDraw {
    pub at: Vec2,
    pub clip: Rect2,
    pub font: Font,
    pub size: f32,
    pub wrap: Wrap,
    pub align: Align,
    pub outline: Outline,
    pub runs: Vec<Run>,
}
