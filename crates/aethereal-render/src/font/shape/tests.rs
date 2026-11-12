//! Shaping the fixture font: advances, kerning, lines, and characters it lacks.

use super::*;
use crate::font::tests::noto;

#[test]
fn each_glyph_is_set_after_the_one_before_by_its_advance_and_their_kerning() {
    // Catches kerning left out, applied after the glyph rather than before it, or applied across a line break: "AV"
    // would sit apart, and a line's first glyph would be pushed by the last glyph of the line before.
    let font = noto();
    let font = font.metrics();
    let shaped = font.shape("AV\nVA");
    let a = font.glyph('A').expect("the fixture draws A");
    let v = font.glyph('V').expect("the fixture draws V");
    let advance = |glyph| i32::from(font.advance(glyph).expect("the glyph advances"));

    let placed = shaped.glyphs.iter().map(|glyph| (glyph.glyph, glyph.x, glyph.line)).collect::<Vec<_>>();

    let expected =
        vec![(a, 0, 0), (v, advance(a) + font.kerning(a, v), 0), (v, 0, 1), (a, advance(v) + font.kerning(v, a), 1)];
    assert_eq!(placed, expected);
    assert_eq!(shaped.lines.iter().map(|line| line.glyphs.clone()).collect::<Vec<_>>(), vec![0..2, 2..4]);
}

#[test]
fn a_character_the_font_lacks_is_drawn_missing_and_listed() {
    // Catches a lacking character dropped, so the text silently loses it, or not listed, so no fallback font is ever
    // tried for it; and a control character drawn as a box.
    let font = noto();
    let font = font.metrics();
    let shaped = font.shape("a\u{4e2d}\tb");

    let glyphs = shaped.glyphs.iter().map(|glyph| (glyph.glyph, glyph.source)).collect::<Vec<_>>();

    let expected = vec![
        (font.glyph('a').expect("the fixture draws a"), 0),
        (GlyphId::MISSING, 1),
        (font.glyph('b').expect("the fixture draws b"), 5),
    ];
    assert_eq!(glyphs, expected);
    assert_eq!(shaped.missing, vec![Missing { character: '\u{4e2d}', source: 1 }]);
}
