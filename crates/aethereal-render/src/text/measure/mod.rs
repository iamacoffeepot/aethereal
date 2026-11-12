//! Measuring a text without placing it (ADR-0052): the box the renderer lays a text out in, from its font's metrics
//! and its fallbacks', by the same function the renderer lays it out with. Pure, so an actor that holds the metrics
//! measures in its own turn, with no mail.

#[cfg(test)]
mod tests;

use super::layout::lay_out;
use super::{Align, Run, Wrap};
use crate::font::Metrics;

/// The box a text is laid out in, in the pixels of the frames it is drawn in, from the first line's top left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measured {
    /// How wide its lines are set within: the wrap width when it wraps, and its widest line's, trailing spaces left
    /// out, when it does not.
    pub width: f32,
    /// How tall its lines are: their count times its own font's line height, ascender to descender and the gap
    /// after, which its fallbacks do not change.
    pub height: f32,
    /// How many lines it has, of which there is always at least one.
    pub lines: u32,
}

/// Measures `runs` laid out as the renderer lays out a text of them (see [`CreateText`](super::CreateText)): in the
/// font of the metrics `own` at `size` pixels to the em, a character it lacks looked for in the fonts of `fallbacks` in
/// order, its lines broken as `wrap` says. `align` sets each line within the box and does not change it. A text's
/// outline does not change the box either: a rim draws up to its width outside it.
#[must_use]
pub fn measure(own: &Metrics, fallbacks: &[Metrics], size: f32, runs: &[Run], wrap: Wrap, align: Align) -> Measured {
    let laid = lay_out(own, fallbacks, size, runs, (wrap, align));
    // A line's height is some thousands of font units, and a text's lines at most some tens of thousands, which an f32
    // holds exactly.
    #[allow(clippy::cast_precision_loss)]
    let height = own.line_metrics().line_height() as f32 * own.pixels_per_unit(size) * laid.lines as f32;

    Measured { width: laid.width, height, lines: laid.lines }
}
