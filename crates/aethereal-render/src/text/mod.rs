//! Text (ADR-0052): fonts made from the renderer's own or the bytes of TrueType files, each with fallbacks fixed as it
//! is made, texts laid out from runs in a font at a size, and texts placed in the passes of frames at a point in their
//! pixels. The renderer draws every placed text with its own glyph program, from one atlas of glyphs' distance fields
//! that it fills, in jobs off its turn, as texts need them. A font's metrics are handed to whoever asks, so an actor
//! measures a text, laid out as the renderer lays it out, in its own turn. What is asked is checked here; what is kept
//! is the render root's.

pub(crate) mod atlas;
pub(crate) mod faces;
pub mod generate;
pub(crate) mod glyph_program;
pub(crate) mod layout;
mod measure;
pub(crate) mod placement;
#[cfg(test)]
mod tests;

use crate::font::FontRefused;
use crate::font::msdf::{BASE_PIXELS_PER_EM, DEFAULT_PADDING};
use crate::frame::Frame;
use crate::pass::{Pass, PassName};
use crate::placement::{JoinRefused, PlacementId};
use aethereal_actor::Bytes;
use aethereal_data::{Bound, Plain};
use aethereal_math::{Rect2, Vec2};
pub use measure::{Measured, measure};

/// Asks the renderer for a font made from `source`, a character it lacks looked for in each of `fallbacks` in turn,
/// fonts made before it. The fallbacks are fixed for the font's life: a fallback released later changes nothing about
/// it, and a fallback's own fallbacks are not tried. Answered with [`FontMade`].
#[derive(Debug, Bound)]
#[kind(name = "aether.render.create_font")]
pub struct CreateFont {
    pub source: FontSource,
    pub fallbacks: Vec<FontId>,
}

/// What a font is made from.
#[derive(Debug, Bound)]
#[kind(name = "aether.render.font_source")]
pub enum FontSource {
    /// The renderer's own proportional font, whose glyphs are shared, not read again.
    Sans,
    /// The renderer's own monospace font, whose glyphs are shared, not read again.
    Mono,
    /// The bytes of a TrueType file, read in the render root's turn.
    Bytes(Bytes),
}

/// A font the renderer made, which only its maker may release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Plain)]
#[kind(name = "aether.render.font_id")]
pub struct FontId {
    pub(crate) id: u64,
}

/// What the renderer answers [`CreateFont`] with: the font, or why it was not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.font_made")]
pub enum FontMade {
    Made(FontId),
    Refused(FontNotMade),
}

/// Why a font was not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.font_not_made")]
pub enum FontNotMade {
    /// Its bytes were not read as a TrueType font: the refusal names the table that was wrong.
    Read(FontRefused),
    /// Its fallback at `fallback`, counted from zero, is not kept: never made, or released.
    Fallback { fallback: u32 },
    /// Its metrics would take the render root past its share of the budget for blobs.
    Unstored,
}

/// Asks the renderer for the metrics of `font` and its fallbacks, to measure text in it as it is laid out (see
/// [`measure`]). Answered with [`FontMetrics`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.read_metrics")]
pub struct ReadMetrics {
    pub font: Font,
}

/// What the renderer answers [`ReadMetrics`] with: the metrics of the font's own face and of each of its fallbacks', in
/// the order tried, each a blob [`Metrics::decode`](crate::font::Metrics::decode) reads. Metrics are tens of kilobytes,
/// past what one piece of mail holds, so each is encoded once, as its face is read, and every answer hands over the
/// same blobs.
#[derive(Debug, Bound)]
#[kind(name = "aether.render.font_metrics")]
pub enum FontMetrics {
    Read { own: Bytes, fallbacks: Vec<Bytes> },
    Refused(MetricsRefused),
}

/// Why a font's metrics were not answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.metrics_refused")]
pub enum MetricsRefused {
    /// No font is kept as the one named: never made, or released.
    Font,
}

/// The font a text is laid out in: one of the renderer's own two, which have no fallbacks, or one an actor made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Plain)]
#[kind(name = "aether.render.font")]
pub enum Font {
    /// The renderer's own proportional font.
    Sans,
    /// The renderer's own monospace font.
    Mono,
    /// A font an actor made.
    Made(FontId),
}

/// Asks the renderer for a text: `runs` laid out in `font` at `size` pixels to the em, in the pixels of the frames it
/// is drawn in, its lines broken as `wrap` says and set as `align` says, each glyph drawn with a rim as `outline` says.
/// Answered with [`TextMade`].
#[derive(Debug, Clone, PartialEq, Plain)]
#[kind(name = "aether.render.create_text")]
pub struct CreateText {
    pub font: Font,
    pub size: f32,
    pub runs: Vec<Run>,
    pub wrap: Wrap,
    pub align: Align,
    pub outline: Outline,
}

/// A part of a text drawn in one colour. `\n` breaks a line, and other control characters are not drawn.
#[derive(Debug, Clone, PartialEq, Plain)]
#[kind(name = "aether.render.run")]
pub struct Run {
    pub text: String,
    pub colour: Colour,
}

impl Run {
    /// `text`, drawn in `colour`.
    #[must_use]
    pub fn new(text: &str, colour: Colour) -> Self {
        Self { text: String::from(text), colour }
    }
}

/// A colour in linear light, as the renderer's colours are, with its alpha: each from 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
#[kind(name = "aether.render.colour")]
pub struct Colour {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
    pub alpha: f32,
}

impl Colour {
    /// Opaque white.
    pub const WHITE: Self = Self { red: 1.0, green: 1.0, blue: 1.0, alpha: 1.0 };

    /// Opaque black.
    pub const BLACK: Self = Self { red: 0.0, green: 0.0, blue: 0.0, alpha: 1.0 };

    /// No colour at all: what an outline that is not drawn is drawn in.
    pub const CLEAR: Self = Self { red: 0.0, green: 0.0, blue: 0.0, alpha: 0.0 };

    /// The opaque colour of `red`, `green`, and `blue`.
    #[must_use]
    pub const fn opaque(red: f32, green: f32, blue: f32) -> Self {
        Self { red, green, blue, alpha: 1.0 }
    }

    /// Its four channels, in order.
    #[must_use]
    pub const fn channels(self) -> [f32; 4] {
        [self.red, self.green, self.blue, self.alpha]
    }
}

/// How a text's lines are broken.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
#[kind(name = "aether.render.wrap_text")]
pub enum Wrap {
    /// Only at `\n`.
    None,
    /// Also at spaces, so each line fits `width` pixels where it can: a word wider than that is left whole.
    At { width: f32 },
}

/// Where each line of a text is set: within its widest line, or within the wrap width when it wraps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.align")]
pub enum Align {
    Left,
    Centre,
    Right,
}

/// A rim drawn around each glyph of a text, or none.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
#[kind(name = "aether.render.outline")]
pub enum Outline {
    None,
    /// A rim `width` pixels wide outside each glyph's edge, in `colour`.
    Of {
        width: f32,
        colour: Colour,
    },
}

/// A text the renderer made, which only its maker may write or release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Plain)]
#[kind(name = "aether.render.text_id")]
pub struct TextId {
    pub(crate) id: u64,
}

/// What the renderer answers [`CreateText`] with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.text_made")]
pub enum TextMade {
    Made(TextId),
    Refused(TextRefused),
}

/// Why a text was not made, or a write was not laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.text_refused")]
pub enum TextRefused {
    /// Its size is not from 1 to 1024 pixels.
    Size,
    /// Its outline is wider than [`widest_outline`] allows at its size, `size` × 4 / 48 − 0.5 pixels and never below
    /// zero, or not a width at all.
    Outline,
    /// Its wrap width is not a width above zero.
    Wrap,
    /// No font is kept as the one named: never made, or released.
    Font,
    /// Its runs hold `bytes` bytes of text, more than the 64 KiB a text may hold.
    Runs { bytes: u64 },
}

/// Lays the text `text` out again of `runs`, in its font, size, wrap, alignment, and outline. It has no answer: a
/// write refused is logged, and the text keeps what it had. Its placements draw the text as it was until every glyph
/// of the new one is in the atlas, then the new one.
#[derive(Debug, Clone, PartialEq, Plain)]
#[kind(name = "aether.render.write_text")]
pub struct WriteText {
    pub text: TextId,
    pub runs: Vec<Run>,
}

/// Asks the renderer to draw `text` in every pass `joins` names, where its maker's list holds it (ADR-0055), its first
/// line's top left at `at` in the pixels of
/// the frame it is drawn in, as `layer` says, each run's alpha multiplied by `opacity`, nothing drawn outside the box
/// `clip` names. Answered with [`TextPlaced`].
#[derive(Debug, Clone, PartialEq, Plain)]
#[kind(name = "aether.render.place_text")]
pub struct PlaceText {
    pub text: TextId,
    pub at: Vec2,
    pub layer: Layer,
    pub opacity: f32,
    pub clip: Clip,
    pub joins: Vec<TextJoin>,
}

/// How a placed text is drawn among what its pass drew.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
#[kind(name = "aether.render.layer")]
pub enum Layer {
    /// Over it, testing no depth.
    Over,
    /// At `depth`, from 0 near to 1 far: in a pass that writes depth, tested and written there; in one that tests it,
    /// tested; in one that holds none, over what is drawn.
    At { depth: f32 },
}

/// What of a placed text is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
#[kind(name = "aether.render.clip")]
pub enum Clip {
    /// All of it.
    None,
    /// Only what falls inside this box, in the frame's pixels from its top left.
    To(Rect2),
}

/// One pass a placed text draws in: the pass `pass` of `frame`, where its maker's
/// [`DrawList`](crate::draw_list::DrawList) holds it (ADR-0055). It names no program and no bindings: the renderer
/// draws a text with its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.text_join")]
pub struct TextJoin {
    pub frame: Frame,
    pub pass: PassName,
}

impl TextJoin {
    /// The pass `P` of `frame`: the pass named by its type.
    #[must_use]
    pub const fn to<P: Pass>(frame: Frame) -> Self {
        Self { frame, pass: PassName::of::<P>() }
    }
}

/// What the renderer answers [`PlaceText`] with: a placement, listed and released as any other is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.text_placed")]
pub enum TextPlaced {
    Made(PlacementId),
    Refused(TextPlacementRefused),
}

/// Why a text's placement was not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.text_placement_refused")]
pub enum TextPlacementRefused {
    /// No text is kept under the id: never made, or released.
    Text,
    /// It joins no pass, and would draw nothing.
    Unjoined,
    /// Its point is not finite.
    At,
    /// Its opacity is not from 0 to 1.
    Opacity,
    /// Its layer's depth is not from 0 to 1.
    Depth,
    /// Its clip is not a box of finite corners.
    Clip,
    /// Its join at `join`, counted from zero, was refused: no such frame or pass that draws, the same pass joined
    /// twice, a pass that reads targets, which the glyph program reads none of, or one whose colour is whole numbers,
    /// which nothing blends into.
    Join { join: u32, why: JoinRefused },
}

/// Moves the placed text `placement`: where it is drawn, as what layer, how opaque, and clipped how, as
/// [`PlaceText`] says. The text is not laid out again. It has no answer: only the placement's maker moves it, and a
/// move refused is logged, the placement left where it was.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
#[kind(name = "aether.render.move_text")]
pub struct MoveText {
    pub placement: PlacementId,
    pub at: Vec2,
    pub layer: Layer,
    pub opacity: f32,
    pub clip: Clip,
}

/// The smallest and largest size of a text, in pixels to the em.
pub const SIZES: (f32, f32) = (1.0, 1024.0);

/// The most bytes of text a text's runs may hold together: 64 KiB.
pub const MOST_RUN_BYTES: u64 = 64 * 1024;

/// The widest outline a text of `size` pixels to the em may have: half a pixel short of how far its glyphs' fields
/// reach past their edges at that size, [`DEFAULT_PADDING`] pixels at [`BASE_PIXELS_PER_EM`], and never below zero.
/// The glyph program smooths a rim's outer edge over the half pixel either side of it, so a rim as wide as the reach
/// would half cover every pixel where the field holds only its farthest distance, a box over the whole quad.
#[must_use]
pub fn widest_outline(size: f32) -> f32 {
    // The padding is a few pixels, which an f32 holds exactly.
    #[allow(clippy::cast_precision_loss)]
    let reach = DEFAULT_PADDING as f32 * size / BASE_PIXELS_PER_EM;
    (reach - SMOOTHED_OUTSIDE).max(0.0)
}

/// How far outside a rim's outer edge the glyph program smooths it, in pixels.
const SMOOTHED_OUTSIDE: f32 = 0.5;

/// Checks a text before it is made: its size, its runs, its wrap width, its outline against its size, and that its
/// font is kept, as `has_font` says.
///
/// # Errors
///
/// Why it would not be made as asked.
pub(crate) fn check(create: &CreateText, has_font: bool) -> Result<(), TextRefused> {
    let (smallest, largest) = SIZES;
    let sized = (smallest..=largest).contains(&create.size);
    if !sized {
        return Err(TextRefused::Size);
    }
    check_runs(&create.runs)?;
    if let Wrap::At { width } = create.wrap {
        let wraps = width.is_finite() && width > 0.0;
        if !wraps {
            return Err(TextRefused::Wrap);
        }
    }
    if let Outline::Of { width, .. } = create.outline {
        let fits = (0.0..=widest_outline(create.size)).contains(&width);
        if !fits {
            return Err(TextRefused::Outline);
        }
    }
    if !has_font {
        return Err(TextRefused::Font);
    }

    Ok(())
}

/// Checks that `runs` hold at most [`MOST_RUN_BYTES`] of text together.
///
/// # Errors
///
/// [`TextRefused::Runs`] when they hold more.
pub(crate) fn check_runs(runs: &[Run]) -> Result<(), TextRefused> {
    let bytes: u64 = runs.iter().map(|run| run.text.len() as u64).sum();
    if bytes > MOST_RUN_BYTES {
        return Err(TextRefused::Runs { bytes });
    }

    Ok(())
}
