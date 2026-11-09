//! `hhea` and `hmtx`: how the font spaces its lines, and how far each glyph advances the pen.

use super::{Data, FontRefused};
use crate::font::LineMetrics;

/// What is read of `hhea`.
pub struct Horizontal {
    /// How the font spaces its lines.
    pub lines: LineMetrics,
    /// How many glyphs have metrics of their own in `hmtx`: every glyph after them advances as the last of them.
    pub metrics: u16,
}

/// Reads `hhea`.
pub fn read(data: Data<'_>) -> Result<Horizontal, FontRefused> {
    let lines = LineMetrics { ascender: data.i16(4)?, descender: data.i16(6)?, line_gap: data.i16(8)? };
    let metrics = data.u16(34)?;
    if metrics == 0 {
        return Err(data.malformed());
    }

    Ok(Horizontal { lines, metrics })
}

/// Reads each of `glyph_count` glyphs' advance from `hmtx`, in which the first `metrics` glyphs have one of their own.
pub fn advances(data: Data<'_>, metrics: u16, glyph_count: u16) -> Result<Vec<u16>, FontRefused> {
    let own = usize::from(metrics.min(glyph_count));
    let mut advances = (0..own).map(|glyph| data.u16(glyph * 4)).collect::<Result<Vec<_>, FontRefused>>()?;

    let refused = data.malformed();
    let last = advances.last().copied().ok_or(refused)?;
    advances.resize(usize::from(glyph_count), last);

    Ok(advances)
}
