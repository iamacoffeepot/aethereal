//! Colour glyphs: read from the renderer's own emoji font, Noto Color Emoji, whose index and images the reader must
//! find, and shrunk from PNGs built here, whose clear pixels must not bleed into the colour kept.

use super::*;
use crate::font::{Font, GlyphId};
use crate::text::faces::EMOJI;

/// A PNG of `width` by `height` pixels, `rgba` four bytes each, row by row.
fn png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let Ok(mut writer) = encoder.write_header() else {
        panic!("the header is written");
    };
    let written = writer.write_image_data(rgba);
    assert!(written.is_ok(), "the image is written: {written:?}");
    drop(writer);

    bytes
}

/// The image `png` holds, set at the origin, in a strike of `pixels_per_em`.
const fn bitmap(png: &[u8], pixels_per_em: u8) -> Bitmap<'_> {
    Bitmap { png, metrics: BitmapMetrics { left: 0, top: 0 }, pixels_per_em }
}

#[test]
fn an_emoji_is_read_as_its_image_set_where_its_strike_says() {
    // Catches the index's offsets read from the wrong place, or against the wrong base, or an image's small metrics
    // misread: a grinning face would draw another emoji, no image, or be set off its baseline. The glyph number, the
    // image's size and bearings, and its PNG's length were read apart with Python.
    let Ok(font) = Font::read(EMOJI) else {
        panic!("the renderer's emoji font reads");
    };
    let grinning = font.metrics().glyph('\u{1f600}');
    assert_eq!(grinning, Some(GlyphId(883)));

    let Some(Drawing::Bitmap(image)) = font.drawing(GlyphId(883)) else {
        panic!("the grinning face is drawn by its image");
    };
    assert_eq!((image.png.len(), image.metrics, image.pixels_per_em), (3305, BitmapMetrics { left: 0, top: 101 }, 109));
    assert!(matches!(font.drawing(GlyphId(3)), Some(Drawing::Blank)), "a space has no image");

    let shrunk = image.shrunk(48.0).map(|shrunk| (shrunk.width, shrunk.height, shrunk.left, shrunk.top));
    // 136 by 128 pixels at 109 to the em, its top 101 above the baseline, at 48 to the em.
    assert_eq!(shrunk, Ok((60, 57, 0, 44)));
}

#[test]
fn a_shrunk_image_keeps_colour_only_where_it_was_drawn() {
    // Catches a pixel's colour weighed without its alpha, so a clear pixel's colour, often black or anything at all,
    // bleeds into an emoji's edge as it is shrunk; a weight lost where a pixel is covered in part; and a pixel past
    // the image's edge counted as anything but clear.
    let red = [255, 0, 0, 255];
    let clear_green = [0, 255, 0, 0];
    let halves = png(4, 2, &[red, red, clear_green, clear_green, red, red, clear_green, clear_green].concat());
    let square = png(2, 2, &[red, clear_green, red, clear_green].concat());
    let solid = png(4, 3, &[red; 12].concat());

    let in_two = bitmap(&halves, 100).shrunk(50.0).map(|shrunk| shrunk.pixels);
    let whole = bitmap(&square, 100).shrunk(50.0).map(|shrunk| shrunk.pixels);
    // Three of the strike's pixels to one: the second covers the last red column and two columns past the edge.
    let thirds = bitmap(&solid, 90).shrunk(30.0).map(|shrunk| shrunk.pixels);

    assert_eq!(in_two, Ok([red, [0, 0, 0, 0]].concat()));
    assert_eq!(whole, Ok(vec![128, 0, 0, 128]));
    assert_eq!(thirds, Ok(vec![255, 0, 0, 255, 85, 0, 0, 85]));
}
