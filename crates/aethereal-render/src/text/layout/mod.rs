//! Laying a text out (ADR-0052): its runs shaped in its font, each character the font lacks looked for in the
//! fallbacks in turn and drawn as the font's missing glyph when none has it, its lines broken at `\n` and, when it
//! wraps, at spaces, and each line set as it is aligned. Positions are in the pixels of the frame it is drawn in, from
//! the first line's top left, `y` down. It reads only the fonts' metrics, so the render root lays a text out to draw
//! it and an actor lays it out to measure it with the one function.

#[cfg(test)]
mod tests;

use super::{Align, Colour, Run, Wrap};
use crate::font::shape::Shaped;
use crate::font::{GlyphId, Metrics};
use aethereal_math::Vec2;
use core::ops::Range;

/// A text laid out: each glyph it draws, where, and in what colour, the characters no font had, and how many lines it
/// has and how wide the widest is. The default lays out nothing.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Laid {
    pub glyphs: Vec<LaidGlyph>,
    /// Each character no font had, once, in the order first met: each is drawn as its font's missing glyph.
    pub missing: Vec<char>,
    /// How many lines it has, of which there is always at least one.
    pub lines: u32,
    /// How wide its widest line is, trailing spaces left out, wrapped or not: its lines are set within the wrap width
    /// when it wraps, which may be wider.
    pub width: f32,
}

/// One glyph of a laid text: which of its fonts draws it, which glyph of that font, its origin on its baseline, and
/// its run's colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaidGlyph {
    pub font: Which,
    pub glyph: GlyphId,
    pub origin: Vec2,
    pub colour: Colour,
}

/// Which of the fonts a text was laid out in draws a glyph: its own, or the fallback at an index of the list it was
/// laid out with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Which {
    Own,
    Fallback(usize),
}

/// One shaped glyph as it is set: the font and glyph that draw it, how far it moves the pen in pixels, and the
/// character it draws.
#[derive(Debug, Clone, Copy)]
struct Set {
    font: Which,
    glyph: GlyphId,
    advance: f32,
    character: char,
}

impl Set {
    /// Whether it is set in the text's own font.
    fn own(self) -> bool {
        self.font == Which::Own
    }
}

/// Where a wrapped line may break: after the last space met on it, or nowhere yet.
#[derive(Debug, Clone, Copy)]
enum Break {
    Unseen,
    After(usize),
}

/// A line as it is drawn: which of the set glyphs are on it, how far along its hard line it starts, and how wide its
/// glyphs are, trailing spaces left out.
#[derive(Debug, Clone)]
struct Broken {
    glyphs: Range<usize>,
    offset: f32,
    width: f32,
}

/// Lays `runs` out in the font of the metrics `own` at `size` pixels to the em, a character it lacks looked for in the
/// fonts of `fallbacks` in order, and not in theirs, its lines broken as `wrap` says and set as `align` says.
pub fn lay_out<M: AsRef<Metrics>>(
    own: &Metrics,
    fallbacks: &[M],
    size: f32,
    runs: &[Run],
    (wrap, align): (Wrap, Align),
) -> Laid {
    let text: String = runs.iter().map(|run| run.text.as_str()).collect();
    let shaped = own.shape(&text);
    let scale = own.pixels_per_unit(size);
    let mut missing = Vec::new();
    let sets: Vec<Set> = shaped
        .glyphs
        .iter()
        .map(|glyph| {
            let character = text.get(glyph.source..).and_then(|rest| rest.chars().next()).unwrap_or('\u{fffd}');
            set(own, fallbacks, (character, glyph.glyph), size, &mut missing)
        })
        .collect();
    let starts = pens(&shaped, &sets, scale);
    let broken: Vec<Broken> =
        shaped.lines.iter().flat_map(|line| broken(&starts, &sets, line.glyphs.clone(), wrap)).collect();

    let widest = broken.iter().map(|line| line.width).fold(0.0, f32::max);
    let within = match wrap {
        Wrap::None => widest,
        Wrap::At { width } => width,
    };
    let metrics = own.line_metrics();
    let ascender = f32::from(metrics.ascender) * scale;
    // A line's height is some thousands of font units, which an f32 holds exactly.
    #[allow(clippy::cast_precision_loss)]
    let line_height = metrics.line_height() as f32 * scale;
    let colours = colours(runs);

    let mut glyphs = Vec::with_capacity(sets.len());
    for (line, at) in broken.iter().zip(0_u16..) {
        let shift = match align {
            Align::Left => 0.0,
            Align::Centre => (within - line.width) / 2.0,
            Align::Right => within - line.width,
        };
        let baseline = f32::from(at).mul_add(line_height, ascender);
        for index in line.glyphs.clone() {
            let drawn = &sets[index];
            if drawn.character.is_whitespace() {
                continue;
            }
            let origin = Vec2::new(shift + starts[index] - line.offset, baseline);
            let colour = colour_at(&colours, shaped.glyphs[index].source);
            glyphs.push(LaidGlyph { font: drawn.font, glyph: drawn.glyph, origin, colour });
        }
    }
    let lines = u32::try_from(broken.len()).unwrap_or(u32::MAX);

    Laid { glyphs, missing, lines, width: widest }
}

/// How `character`, which `own` shaped as `shaped`, is set: in `own` when it has the character; else in the first of
/// `fallbacks` that has it; else as `own`'s missing glyph, noted in `missing` the first time.
fn set<M: AsRef<Metrics>>(
    own: &Metrics,
    fallbacks: &[M],
    (character, shaped): (char, GlyphId),
    size: f32,
    missing: &mut Vec<char>,
) -> Set {
    let advance =
        |font: &Metrics, glyph: GlyphId| f32::from(font.advance(glyph).unwrap_or(0)) * font.pixels_per_unit(size);
    let shaped_missing = shaped == GlyphId::MISSING;
    let unmapped = own.glyph(character).is_none();
    let lacks = shaped_missing && unmapped;
    if !lacks {
        return Set { font: Which::Own, glyph: shaped, advance: advance(own, shaped), character };
    }
    let fallen = fallbacks.iter().map(AsRef::as_ref).enumerate().find_map(|(at, font)| {
        let glyph = font.glyph(character)?;
        Some(Set { font: Which::Fallback(at), glyph, advance: advance(font, glyph), character })
    });
    if let Some(fallen) = fallen {
        return fallen;
    }
    if !missing.contains(&character) {
        missing.push(character);
    }

    Set { font: Which::Own, glyph: GlyphId::MISSING, advance: advance(own, GlyphId::MISSING), character }
}

/// Where each glyph's pen is along its hard line, in pixels: two neighbours both of the text's own font as it shaped
/// them, kerned, and a glyph of a fallback, or before one, by its own advance.
fn pens(shaped: &Shaped, sets: &[Set], scale: f32) -> Vec<f32> {
    let mut starts = vec![0.0; sets.len()];
    for line in &shaped.lines {
        let mut pen = 0.0;
        for index in line.glyphs.clone() {
            starts[index] = pen;
            let next = index + 1;
            let kerned = next < line.glyphs.end && sets[index].own() && sets[next].own();
            // A pen is exact to the font unit for lines up to 2^24 units, far longer than any drawn.
            #[allow(clippy::cast_precision_loss)]
            let moved = if kerned {
                (shaped.glyphs[next].x - shaped.glyphs[index].x) as f32 * scale
            } else {
                sets[index].advance
            };
            pen += moved;
        }
    }

    starts
}

/// The lines the hard line of the set glyphs `glyphs` is drawn as: itself, or, when it wraps at `wrap`, broken after
/// the last space before each glyph that would end past the width. A word wider than the width is left whole.
fn broken(starts: &[f32], sets: &[Set], glyphs: Range<usize>, wrap: Wrap) -> Vec<Broken> {
    let mut lines = Vec::new();
    let mut first = glyphs.start;
    if let Wrap::At { width } = wrap {
        let mut after = Break::Unseen;
        for index in glyphs.clone() {
            if sets[index].character == ' ' {
                after = Break::After(index);
                continue;
            }
            let right = starts[index] + sets[index].advance - starts.get(first).copied().unwrap_or(0.0);
            let over = right > width;
            if let Break::After(space) = after
                && over
            {
                lines.push(first..space + 1);
                first = space + 1;
                after = Break::Unseen;
            }
        }
    }
    lines.push(first..glyphs.end);

    lines
        .into_iter()
        .map(|line| {
            let offset = starts.get(line.start).copied().unwrap_or(0.0);
            let width = line
                .clone()
                .filter(|index| !sets[*index].character.is_whitespace())
                .map(|index| starts[index] + sets[index].advance - offset)
                .fold(0.0, f32::max);
            Broken { glyphs: line, offset, width }
        })
        .collect()
}

/// Where each run ends in the text the runs make together, in bytes, beside its colour.
fn colours(runs: &[Run]) -> Vec<(usize, Colour)> {
    let mut end = 0;

    runs.iter()
        .map(|run| {
            end += run.text.len();
            (end, run.colour)
        })
        .collect()
}

/// The colour of the run the byte `source` of the text is in.
fn colour_at(colours: &[(usize, Colour)], source: usize) -> Colour {
    colours.iter().find(|(end, _)| source < *end).map_or(Colour::WHITE, |(_, colour)| *colour)
}
