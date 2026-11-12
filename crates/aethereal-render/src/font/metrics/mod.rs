//! A font's metrics (ADR-0052): all a text is laid out in it by, and nothing it is drawn by. Its units per em, its
//! line spacing, each glyph's advance, which glyph draws each character, and the pairs it kerns. An actor that measures
//! text holds them as a value, handed to it as bytes the render root encoded once (`encode`), and decodes them
//! (`decode`) into exactly the value the render root lays out with.

pub(super) mod codec;
#[cfg(test)]
mod tests;

use super::kerning::Kerning;
use super::tables::CharacterMap;
use super::{GlyphId, LineMetrics, UNITS_PER_EM};
use codec::{Reader, Writer};

/// A font's metrics: its units per em, its line spacing, each glyph's advance, its character map, and the pairs it
/// kerns. Glyph 0, the missing glyph, always has an advance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metrics {
    units_per_em: u16,
    lines: LineMetrics,
    advances: Vec<u16>,
    characters: CharacterMap,
    kerning: Kerning,
}

/// Why bytes were not decoded as a font's metrics, naming what was wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricsMalformed {
    /// The bytes end before what they say they hold.
    CutShort,
    /// Bytes are left over past the metrics.
    Trailing,
    /// The units per em are not from 16 to 16384, as a font's are.
    Units,
    /// There is no glyph, or more than a font numbers.
    Advances,
    /// The character map's groups are out of order, overlap, reach past Unicode, or name a glyph past the font's.
    Characters,
    /// The kerning's pairs, glyphs, or classes are out of order or listed twice, or a subtable is of no form there is.
    Kerning,
}

/// The most glyphs a font numbers: glyph numbers are 16 bits.
const MOST_GLYPHS: usize = 65_536;

impl Metrics {
    /// The metrics of a font read: what its `head`, `hhea`, `hmtx`, `cmap`, and kerning say.
    pub(super) const fn new(
        units_per_em: u16,
        lines: LineMetrics,
        advances: Vec<u16>,
        characters: CharacterMap,
        kerning: Kerning,
    ) -> Self {
        Self { units_per_em, lines, advances, characters, kerning }
    }

    /// How many font units make one em: the size a text is asked for at.
    #[must_use]
    pub const fn units_per_em(&self) -> u16 {
        self.units_per_em
    }

    /// How the font spaces its lines.
    #[must_use]
    pub const fn line_metrics(&self) -> LineMetrics {
        self.lines
    }

    /// How many pixels one font unit is at a size of `pixels_per_em`.
    #[must_use]
    pub fn pixels_per_unit(&self, pixels_per_em: f32) -> f32 {
        pixels_per_em / f32::from(self.units_per_em)
    }

    /// The glyph that draws `character`, if the font has one.
    #[must_use]
    pub fn glyph(&self, character: char) -> Option<GlyphId> {
        self.characters.glyph(u32::from(character)).map(GlyphId)
    }

    /// How far the pen moves past `glyph`, in font units, if the font has that glyph.
    #[must_use]
    pub fn advance(&self, glyph: GlyphId) -> Option<u16> {
        self.advances.get(usize::from(glyph.0)).copied()
    }

    /// How much closer, or further apart, the font sets `right` after `left`, in font units: negative draws them
    /// together. Zero for a pair the font does not kern.
    #[must_use]
    pub fn kerning(&self, left: GlyphId, right: GlyphId) -> i32 {
        self.kerning.between(left.0, right.0)
    }

    /// The metrics as bytes, which [`Self::decode`] reads back as these metrics, little-endian: the units per em, the
    /// line spacing, the advances, the character map's groups, and the kerning's lookups, each list after its count.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::default();
        writer.u16(self.units_per_em);
        writer.i16(self.lines.ascender);
        writer.i16(self.lines.descender);
        writer.i16(self.lines.line_gap);
        writer.count(self.advances.len());
        for advance in &self.advances {
            writer.u16(*advance);
        }
        self.characters.write(&mut writer);
        self.kerning.write(&mut writer);

        writer.into_bytes()
    }

    /// Reads the metrics [`Self::encode`] wrote. Every count is checked against the bytes left before anything is
    /// taken for it, so no more is held than the bytes could describe, and what would break a lookup (groups or pairs
    /// out of order, a character mapped to a glyph past the font's) is refused.
    ///
    /// # Errors
    ///
    /// Why the bytes are not metrics, naming what was wrong. Bytes cut short or garbled are refused, never a panic.
    pub fn decode(bytes: &[u8]) -> Result<Self, MetricsMalformed> {
        let mut reader = Reader::new(bytes);
        let units_per_em = reader.u16()?;
        if !UNITS_PER_EM.contains(&units_per_em) {
            return Err(MetricsMalformed::Units);
        }
        let lines = LineMetrics { ascender: reader.i16()?, descender: reader.i16()?, line_gap: reader.i16()? };
        let count = reader.count(2)?;
        let glyphs = (1..=MOST_GLYPHS).contains(&count);
        if !glyphs {
            return Err(MetricsMalformed::Advances);
        }
        let advances = (0..count).map(|_| reader.u16()).collect::<Result<Vec<_>, _>>()?;
        let characters = CharacterMap::read_from(&mut reader, advances.len())?;
        let kerning = Kerning::read_from(&mut reader)?;
        reader.end()?;

        Ok(Self { units_per_em, lines, advances, characters, kerning })
    }
}

/// Metrics are their own metrics, so a text is measured with a list of them.
impl AsRef<Self> for Metrics {
    fn as_ref(&self) -> &Self {
        self
    }
}
