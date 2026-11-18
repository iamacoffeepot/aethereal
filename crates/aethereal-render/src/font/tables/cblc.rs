//! `CBLC` and `CBDT`: a colour font's glyphs as PNG images at one or more sizes, each a strike. `CBLC` says where each
//! glyph's image of each strike is in `CBDT`, and `CBDT` holds it, beside where it sits from the glyph's origin. Only
//! the largest strike is read, as the renderer shrinks it to its own size; and of the forms these tables take, only
//! the one Noto Color Emoji is written in: an index of 32-bit offsets (format 1) to images that each hold their own
//! small metrics and a PNG (format 17). Any other form is refused as unsupported, never guessed at.

use super::{Data, FontRefused, Table};
use crate::font::bitmap::{BitmapMetrics, Bitmaps, Image};
use core::ops::Range;

/// The bytes of a strike's record in `CBLC`.
const STRIKE_BYTES: usize = 48;

/// The index's form this reads: a 32-bit offset for each glyph of a range, and one past the last.
const OFFSETS_32: u16 = 1;

/// The image's form this reads: small metrics, then a PNG's length and its bytes.
const SMALL_PNG: u16 = 17;

/// The bits a pixel of a colour strike has: 32, red, green, blue, and alpha.
const COLOUR_DEPTH: u8 = 32;

/// A strike: where its ranges of glyphs are indexed, how many there are, and its size.
struct Strike {
    ranges: usize,
    count: usize,
    pixels_per_em: u8,
}

/// Reads the images of the largest strike `cblc` indexes in `cbdt`, for a font of `glyph_count` glyphs.
pub fn read(cblc: Data<'_>, cbdt: Data<'_>, glyph_count: u16) -> Result<Bitmaps, FontRefused> {
    let strike = largest(cblc)?;
    let mut images = vec![Image::Blank; usize::from(glyph_count)];
    let mut png = Vec::with_capacity(cbdt.len());
    for range in 0..strike.count {
        let at = cblc.at(strike.ranges, range, 8)?;
        let (first, last) = (usize::from(cblc.u16(at)?), usize::from(cblc.u16(at + 2)?));
        let subtable = strike.ranges.checked_add(cblc.offset32(at + 4)?).ok_or_else(|| cblc.malformed())?;
        let ordered = first <= last && last < images.len();
        if !ordered {
            return Err(cblc.malformed());
        }

        let indexed = cblc.u16(subtable)? == OFFSETS_32;
        let small_png = cblc.u16(subtable + 2)? == SMALL_PNG;
        let readable = indexed && small_png;
        if !readable {
            return Err(FontRefused::Unsupported(Table::Cblc));
        }

        let start = cblc.offset32(subtable + 4)?;
        let offset = |nth: usize| {
            let at = cblc.at(subtable + 8, nth, 4)?;
            start.checked_add(cblc.offset32(at)?).ok_or_else(|| cblc.malformed())
        };
        for (nth, image) in images[first..=last].iter_mut().enumerate() {
            let bytes = offset(nth)?..offset(nth + 1)?;
            if bytes.end < bytes.start {
                return Err(cblc.malformed());
            }
            if !bytes.is_empty() {
                *image = image_at(cbdt, bytes, &mut png)?;
            }
        }
    }

    Ok(Bitmaps::new(strike.pixels_per_em, images, png))
}

/// The largest strike `cblc` holds, each of colour images.
fn largest(cblc: Data<'_>) -> Result<Strike, FontRefused> {
    let strikes = (0..cblc.offset32(4)?)
        .map(|strike| {
            let at = cblc.at(8, strike, STRIKE_BYTES)?;
            let depth = cblc.u8(at + 46)?;
            if depth != COLOUR_DEPTH {
                return Err(FontRefused::Unsupported(Table::Cblc));
            }
            Ok(Strike { ranges: cblc.offset32(at)?, count: cblc.offset32(at + 8)?, pixels_per_em: cblc.u8(at + 45)? })
        })
        .collect::<Result<Vec<_>, FontRefused>>()?;

    strikes.into_iter().max_by_key(|strike| strike.pixels_per_em).ok_or_else(|| cblc.malformed())
}

/// The image `cbdt` holds at `bytes`: its small metrics, and its PNG, appended to `png`.
fn image_at(cbdt: Data<'_>, bytes: Range<usize>, png: &mut Vec<u8>) -> Result<Image, FontRefused> {
    let image = cbdt.slice(bytes.start, bytes.len())?;
    // Small metrics: height, width, then the bearings, then the advance, which `hmtx` holds as well.
    let metrics = BitmapMetrics { left: image.i8(2)?, top: image.i8(3)? };
    let length = image.offset32(5)?;
    let start = png.len();
    png.extend_from_slice(image.slice(9, length)?.bytes());

    Ok(Image::Png { metrics, bytes: start..png.len() })
}
