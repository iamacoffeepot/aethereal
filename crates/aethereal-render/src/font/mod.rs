//! Fonts (ADR-0052): a TrueType font read from its bytes into its metrics, which map its characters to glyphs and kern
//! them into lines, and its glyphs: outlines, each made into a multi-channel signed distance field the glyph program
//! draws every size from, or, in a colour emoji font, PNG images, each shrunk to the size the renderer keeps. Pure functions: the render root keeps what they make, and an actor that measures text keeps the
//! metrics the render root hands it.

#[cfg(test)]
mod tests;

pub mod bitmap;
mod data;
mod kerning;
mod metrics;
pub mod msdf;
pub mod outline;
pub mod shape;
mod tables;

use aethereal_data::Plain;
use bitmap::{Bitmap, Bitmaps};
use core::ops::RangeInclusive;
use kerning::Kerning;
pub use metrics::{Metrics, MetricsMalformed};
use msdf::Msdf;
use outline::Outline;
use tables::Directory;

/// A glyph of a font: its number in the font's tables. Glyph 0 is the font's missing glyph, drawn for a character the
/// font lacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GlyphId(pub u16);

/// How many font units an em may be: from 16 to 16384, as `head` allows.
const UNITS_PER_EM: RangeInclusive<u16> = 16..=16384;

impl GlyphId {
    /// The font's missing glyph, drawn for a character the font has no glyph for: usually a box.
    pub const MISSING: Self = Self(0);
}

/// A TrueType font, read whole as it is made: its metrics, which are all a text is laid out by, and every glyph's
/// outline or image, which only drawing it needs. Nothing of the bytes it was read from is kept but its images' PNGs.
#[derive(Debug, Clone)]
pub struct Font {
    metrics: Metrics,
    glyphs: Glyphs,
}

/// How a font's glyphs are drawn: each an outline, in `glyf`, or each an image in colour, in `CBDT`. A font that has
/// both is read as its images: its outlines are what draws it where colour cannot be.
#[derive(Debug, Clone)]
enum Glyphs {
    Outlines(Vec<Outline>),
    Bitmaps(Bitmaps),
}

/// How one glyph of a font is drawn.
#[derive(Debug, Clone, Copy)]
pub enum Drawing<'a> {
    /// By its outline, in the text's colour. A glyph that draws nothing, as a space, has an outline of no contours.
    Outline(&'a Outline),
    /// By its image, in its own colours.
    Bitmap(Bitmap<'a>),
    /// Not at all: a glyph of a colour font that has no image, as a space.
    Blank,
}

/// How a font spaces its lines, in font units, the baseline at zero and `y` up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LineMetrics {
    /// How far the font's tallest glyphs rise above the baseline.
    pub ascender: i16,
    /// How far the font's lowest glyphs fall below the baseline, as a negative number.
    pub descender: i16,
    /// The space the font asks for between one line's descender and the next line's ascender.
    pub line_gap: i16,
}

impl LineMetrics {
    /// How far one line's baseline is below the line before it, in font units.
    #[must_use]
    pub fn line_height(self) -> i32 {
        i32::from(self.ascender) - i32::from(self.descender) + i32::from(self.line_gap)
    }
}

/// Why a font was not read. Each names what could not be read, so a refusal says which table was wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.font_refused")]
pub enum FontRefused {
    /// The bytes are not a TrueType font: they are a container this does not open, or not a font at all.
    Container(Container),
    /// The font's outlines are in a `CFF ` or `CFF2` table, cubic curves, where TrueType's are quadratic in `glyf`.
    Cff,
    /// The font is variable (it has an `fvar` table): its outlines move along axes this does not read.
    Variable,
    /// A table the font needs is not in it.
    Missing(Table),
    /// A table is cut short, points past its own end, or holds a value no font may.
    Malformed(Table),
    /// A table is in a form this does not read: a character map with no Unicode subtable of format 4 or 12.
    Unsupported(Table),
}

/// What the bytes of a font that is not TrueType are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.font_container")]
pub enum Container {
    /// A WOFF file: TrueType or CFF tables, each compressed.
    Woff,
    /// A WOFF2 file: tables compressed together, and transformed.
    Woff2,
    /// A TrueType collection: several fonts in one file.
    Collection,
    /// No font this knows of.
    Unknown,
}

/// A table of a TrueType font, as a refusal names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.font_table")]
pub enum Table {
    /// The table directory at the start of the file, which says where every table is.
    Directory,
    /// `head`: units per em, and the form of `loca`'s offsets.
    Head,
    /// `maxp`: how many glyphs the font has.
    Maxp,
    /// `hhea`: line spacing, and how many glyphs have an advance of their own in `hmtx`.
    Hhea,
    /// `hmtx`: each glyph's advance.
    Hmtx,
    /// `CBLC`: where each colour glyph's image is in `CBDT`, at each size.
    Cblc,
    /// `CBDT`: each colour glyph's image, a PNG.
    Cbdt,
    /// `loca`: where each glyph's outline is in `glyf`.
    Loca,
    /// `glyf`: each glyph's outline, of quadratic curves, or of other glyphs placed.
    Glyf,
    /// `cmap`: which glyph draws each character.
    Cmap,
    /// `kern`: the pairs of glyphs kerned, in the older table.
    Kern,
    /// `GPOS`: glyph positioning, of which the pairs a `kern` feature adjusts are read.
    Gpos,
}

impl Font {
    /// Reads a TrueType font from the bytes of its file.
    ///
    /// # Errors
    ///
    /// Why the font could not be read, naming the table that was wrong. Bytes that are cut short or garbled are
    /// refused, never a panic.
    pub fn read(bytes: &[u8]) -> Result<Self, FontRefused> {
        let directory = Directory::read(bytes)?;
        let head = tables::head::read(directory.table(*b"head", Table::Head)?)?;
        let glyph_count = tables::maxp::read(directory.table(*b"maxp", Table::Maxp)?)?;
        let horizontal = tables::hhea::read(directory.table(*b"hhea", Table::Hhea)?)?;
        let advances =
            tables::hhea::advances(directory.table(*b"hmtx", Table::Hmtx)?, horizontal.metrics, glyph_count)?;
        let glyphs = match directory.optional(*b"CBLC", Table::Cblc)? {
            Some(cblc) => {
                Glyphs::Bitmaps(tables::cblc::read(cblc, directory.table(*b"CBDT", Table::Cbdt)?, glyph_count)?)
            }
            None => Glyphs::Outlines(tables::glyf::read(
                directory.table(*b"loca", Table::Loca)?,
                directory.table(*b"glyf", Table::Glyf)?,
                head.offsets,
                glyph_count,
            )?),
        };
        let characters = tables::cmap::read(directory.table(*b"cmap", Table::Cmap)?, glyph_count)?;

        let positioned = directory.optional(*b"GPOS", Table::Gpos)?.map(tables::gpos::read).transpose()?;
        let kerned = directory.optional(*b"kern", Table::Kern)?.map(tables::kern::read).transpose()?;
        let kerning = Kerning::from_tables(positioned, kerned);

        let metrics = Metrics::new(head.units_per_em, horizontal.lines, advances, characters, kerning);

        Ok(Self { metrics, glyphs })
    }

    /// Its metrics: what a text is laid out in it by.
    #[must_use]
    pub const fn metrics(&self) -> &Metrics {
        &self.metrics
    }

    /// How many glyphs the font has.
    #[must_use]
    pub fn glyph_count(&self) -> usize {
        match &self.glyphs {
            Glyphs::Outlines(outlines) => outlines.len(),
            Glyphs::Bitmaps(bitmaps) => bitmaps.glyph_count(),
        }
    }

    /// How `glyph` is drawn, if the font has that glyph.
    #[must_use]
    pub fn drawing(&self, glyph: GlyphId) -> Option<Drawing<'_>> {
        match &self.glyphs {
            Glyphs::Outlines(outlines) => outlines.get(usize::from(glyph.0)).map(Drawing::Outline),
            Glyphs::Bitmaps(bitmaps) => bitmaps.drawing(usize::from(glyph.0)),
        }
    }

    /// The outline of `glyph`, in font units, if the font has that glyph and draws it by its outline. A glyph that
    /// draws nothing, as a space, has an outline of no contours.
    #[must_use]
    pub fn outline(&self, glyph: GlyphId) -> Option<&Outline> {
        match self.drawing(glyph)? {
            Drawing::Outline(outline) => Some(outline),
            Drawing::Bitmap(_) | Drawing::Blank => None,
        }
    }

    /// The multi-channel signed distance field of `glyph` at `pixels_per_em` (the renderer's atlas takes
    /// [`msdf::BASE_PIXELS_PER_EM`]), its field reaching `padding` pixels past the outline on every side, if the font
    /// has that glyph.
    #[must_use]
    pub fn msdf(&self, glyph: GlyphId, pixels_per_em: f32, padding: u32) -> Option<Msdf> {
        let scale = self.metrics.pixels_per_unit(pixels_per_em);
        self.outline(glyph).map(|outline| msdf::generate(outline, scale, padding))
    }
}
