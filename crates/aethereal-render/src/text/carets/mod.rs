//! Where a caret stands in a text laid out as the renderer lays it out (ADR-0052, ADR-0056: Stock widgets): each line
//! as it is drawn, the bytes of the text it holds, and how far along it each character's boundary is. Pure, from the
//! fonts' metrics and by the layout the renderer draws with, so an actor that edits or selects a text finds where its
//! caret, its selection, and a press stand in its own turn, to the pixel the text is drawn at.

#[cfg(test)]
mod tests;

use super::Wrap;
use super::layout::{broken, pens, sets};
use crate::font::Metrics;

/// A text laid out for its carets: each line as it is drawn, top to bottom, of which there is always at least one, and
/// how tall each is.
#[derive(Debug, Clone, PartialEq)]
pub struct Carets {
    pub lines: Vec<CaretLine>,
    pub line_height: f32,
}

/// One line of a text as it is drawn: the bytes of the text it holds, from `start` to `end`, how it ends, how wide its
/// glyphs are, trailing spaces left out, as its alignment sets it, and where each character's boundary on it stands,
/// in order, the last at `end`.
///
/// A line that ends at a `\n` holds the bytes before it, and the next line starts after it; a line broken where it
/// wraps holds the space it broke after, and its `end` is the next line's `start`. A character no glyph draws, a
/// control character, has no stop of its own: a caret before it stands where the next stop does.
#[derive(Debug, Clone, PartialEq)]
pub struct CaretLine {
    pub start: usize,
    pub end: usize,
    pub ends: Ends,
    pub width: f32,
    pub stops: Vec<Stop>,
}

/// How a line ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ends {
    /// At a `\n`, or at the text's end.
    Hard,
    /// Where it wraps, after a space.
    Soft,
}

/// A character's boundary on a line: its byte in the text, and how far along the line it stands, in pixels from the
/// line's start before its alignment shifts it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stop {
    pub at: usize,
    pub x: f32,
}

/// Lays `text` out for its carets, as [`measure`](super::measure) and the renderer lay it out: in the font of the
/// metrics `own` at `size` pixels to the em, a character it lacks looked for in the fonts of `fallbacks` in order, its
/// lines broken as `wrap` says.
#[must_use]
pub fn carets<M: AsRef<Metrics>>(own: &Metrics, fallbacks: &[M], size: f32, text: &str, wrap: Wrap) -> Carets {
    let shaped = own.shape(text);
    let set = sets(own, fallbacks, size, (text, &shaped), &mut Vec::new());
    let starts = pens(&shaped, &set, own.pixels_per_unit(size));
    // A line's height is some thousands of font units, which an f32 holds exactly.
    #[allow(clippy::cast_precision_loss)]
    let line_height = own.line_metrics().line_height() as f32 * own.pixels_per_unit(size);

    let mut lines = Vec::new();
    let mut hard_start = 0;
    for (hard, line) in text.split('\n').zip(&shaped.lines) {
        let hard_end = hard_start + hard.len();
        let pieces = broken(&starts, &set, line.glyphs.clone(), wrap);
        let first_of = |piece: usize| pieces.get(piece).map(|next| shaped.glyphs[next.glyphs.start].source);
        for (at, piece) in pieces.iter().enumerate() {
            let start = if at == 0 {
                hard_start
            } else {
                first_of(at).unwrap_or(hard_end)
            };
            let (end, ends) = first_of(at + 1).map_or((hard_end, Ends::Hard), |next| (next, Ends::Soft));
            let mut stops: Vec<Stop> = piece
                .glyphs
                .clone()
                .map(|glyph| Stop { at: shaped.glyphs[glyph].source, x: starts[glyph] - piece.offset })
                .collect();
            let reach =
                piece.glyphs.clone().last().map_or(0.0, |glyph| starts[glyph] + set[glyph].advance - piece.offset);
            stops.push(Stop { at: end, x: reach });
            lines.push(CaretLine { start, end, ends, width: piece.width, stops });
        }
        hard_start = hard_end + 1;
    }

    Carets { lines, line_height }
}

impl Carets {
    /// Which line a caret at the byte `at` stands on: the last that starts at or before it, so a caret at a wrapped
    /// line's end stands at the start of the next.
    #[must_use]
    pub fn line_of(&self, at: usize) -> usize {
        self.lines.iter().rposition(|line| line.start <= at).unwrap_or(0)
    }

    /// How far along its line a caret at the byte `at` stands, and which line that is.
    #[must_use]
    pub fn place_of(&self, at: usize) -> (usize, f32) {
        let line = self.line_of(at);
        let x = self.lines.get(line).map_or(0.0, |line| line.x_of(at));

        (line, x)
    }
}

impl CaretLine {
    /// How far along it a caret at the byte `at` stands: at the first stop at or after it.
    #[must_use]
    pub fn x_of(&self, at: usize) -> f32 {
        let found = self.stops.iter().find(|stop| stop.at >= at).or_else(|| self.stops.last());

        found.map_or(0.0, |stop| stop.x)
    }

    /// The last byte a caret stands at on it: its end, or, where it wraps, before the space it broke after, as a caret
    /// at its end stands at the next line's start.
    #[must_use]
    pub fn last(&self) -> usize {
        match self.ends {
            Ends::Hard => self.end,
            Ends::Soft => self.stops.iter().rev().nth(1).map_or(self.start, |stop| stop.at),
        }
    }

    /// The boundary a caret stands at on it nearest `x` pixels along it.
    #[must_use]
    pub fn nearest(&self, x: f32) -> usize {
        let last = self.last();
        let reachable = self.stops.iter().filter(|stop| stop.at <= last);
        let nearest = reachable.min_by(|one, other| (one.x - x).abs().total_cmp(&(other.x - x).abs()));

        nearest.map_or(self.start, |stop| stop.at)
    }
}
