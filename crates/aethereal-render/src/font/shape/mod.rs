//! Shaping, simply (ADR-0052): each character mapped to a glyph by the font's character map, set after the glyph
//! before it by that glyph's advance, and kerned against it. `\n` breaks a line, and other control characters are not
//! drawn. A character the font lacks is drawn as its missing glyph, and listed, so that it can be looked for in another
//! font. Scripts that join or reorder are set in their characters' plain forms.

#[cfg(test)]
mod tests;

use super::{GlyphId, Metrics};
use aethereal_math::Vec2;
use std::ops::Range;

/// A text shaped in one font: its glyphs in order, in font units, its lines, and the characters the font lacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shaped {
    /// Every glyph drawn, in the text's order.
    pub glyphs: Vec<ShapedGlyph>,
    /// Each line, of which there is always at least one.
    pub lines: Vec<Line>,
    /// Each character the font has no glyph for, in the text's order, drawn as [`GlyphId::MISSING`].
    pub missing: Vec<Missing>,
    /// How far each line's baseline is below the one before it, in font units.
    pub line_height: i32,
}

/// A glyph of a shaped text, and where its pen is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapedGlyph {
    pub glyph: GlyphId,
    /// Where in the text, in bytes, the character the glyph draws is.
    pub source: usize,
    /// How far along its line the glyph's origin is, in font units.
    pub x: i32,
    /// Which line the glyph is on, from 0 at the top.
    pub line: u32,
}

/// A line of a shaped text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Which of the text's glyphs are on the line.
    pub glyphs: Range<usize>,
    /// How far the pen moved along the line, its last glyph's advance counted, in font units.
    pub width: i32,
}

/// A character the font has no glyph for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Missing {
    pub character: char,
    /// Where in the text, in bytes, the character is.
    pub source: usize,
}

/// What a line's next glyph is set after.
#[derive(Debug, Clone, Copy)]
enum After {
    /// The start of the line: nothing to kern against.
    LineStart,
    /// This glyph.
    Glyph(GlyphId),
}

impl Metrics {
    /// Shapes `text` in the font these are the metrics of.
    #[must_use]
    pub fn shape(&self, text: &str) -> Shaped {
        let mut glyphs = Vec::new();
        let mut lines = Vec::new();
        let mut missing = Vec::new();
        let mut x = 0;
        let mut line = 0;
        let mut line_start = 0;
        let mut after = After::LineStart;

        for (source, character) in text.char_indices() {
            if character == '\n' {
                lines.push(Line { glyphs: line_start..glyphs.len(), width: x });
                line += 1;
                x = 0;
                line_start = glyphs.len();
                after = After::LineStart;
                continue;
            }
            if character.is_control() {
                continue;
            }

            let glyph = self.glyph(character).unwrap_or_else(|| {
                missing.push(Missing { character, source });
                GlyphId::MISSING
            });
            if let After::Glyph(left) = after {
                x = x.saturating_add(self.kerning(left, glyph));
            }
            glyphs.push(ShapedGlyph { glyph, source, x, line });
            x = x.saturating_add(self.advance(glyph).map_or(0, i32::from));
            after = After::Glyph(glyph);
        }
        lines.push(Line { glyphs: line_start..glyphs.len(), width: x });

        Shaped { glyphs, lines, missing, line_height: self.line_metrics().line_height() }
    }
}

impl ShapedGlyph {
    /// Where the glyph's origin is on its baseline, at `pixels_per_unit` pixels a font unit, from the first line's
    /// origin, `y` up, so that later lines are below it, `line_height` font units apart.
    pub fn pen(self, line_height: i32, pixels_per_unit: f32) -> Vec2 {
        let below = i64::from(self.line) * i64::from(line_height);
        #[allow(
            clippy::cast_precision_loss,
            reason = "a pen is exact to the font unit for lines up to 2^24 units, far longer than any drawn"
        )]
        let units = Vec2::new(self.x as f32, -(below as f32));
        units * pixels_per_unit
    }
}
