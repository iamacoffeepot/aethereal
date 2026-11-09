//! Laying texts out in the renderer's own fonts: where wrapped and aligned lines are set, and which font draws a
//! character the text's own lacks.

use super::*;
use crate::text::Font;
use crate::text::faces::{Faces, FontKey};
use core::slice;

/// A character the renderer's own monospace font draws and its proportional one lacks: a box-drawing line.
const ONLY_MONO: char = '\u{2500}';

/// A character neither of the renderer's own fonts draws: an emoji.
const IN_NEITHER: char = '\u{1f600}';

/// The renderer's own proportional font, and its own monospace one as a font an actor made, under the id 7.
fn faces() -> (Face, Face) {
    let Ok(faces) = Faces::new() else {
        panic!("the renderer's own fonts are read");
    };
    let (Some(sans), Some(mono)) = (faces.face(Font::Sans), faces.face(Font::Mono)) else {
        panic!("the renderer's own fonts are kept");
    };

    (sans, Face { key: FontKey::Made(7), font: mono.font })
}

/// One white run of `text`.
fn white(text: &str) -> Vec<Run> {
    vec![Run::new(text, Colour::WHITE)]
}

/// How far `character`'s glyph moves the pen in `face` at `size`, in pixels.
fn advance(face: &Face, character: char, size: f32) -> f32 {
    let glyph = face.font.glyph(character).unwrap_or(GlyphId::MISSING);

    f32::from(face.font.advance(glyph).unwrap_or(0)) * face.font.pixels_per_unit(size)
}

/// The glyphs of `laid` on the baseline `baseline`, by their origins' `x`, left to right.
fn line_at(laid: &Laid, baseline: f32) -> Vec<f32> {
    let mut xs: Vec<f32> = laid
        .glyphs
        .iter()
        .filter(|glyph| (glyph.origin.y - baseline).abs() < 0.01)
        .map(|glyph| glyph.origin.x)
        .collect();
    xs.sort_by(f32::total_cmp);

    xs
}

/// The baselines of `laid`'s lines, top to bottom.
fn baselines(laid: &Laid) -> Vec<f32> {
    let mut ys: Vec<f32> = laid.glyphs.iter().map(|glyph| glyph.origin.y).collect();
    ys.sort_by(f32::total_cmp);
    ys.dedup_by(|a, b| (*a - *b).abs() < 0.01);

    ys
}

#[test]
fn a_wrapped_line_breaks_after_its_last_space_and_each_line_starts_at_its_own_left() {
    // Catches a line broken mid-word or before the space, and a broken line's glyphs left where they lay on the hard
    // line, so the second line starts far to the right of the first.
    let (sans, _) = faces();
    let size = 20.0;
    let word = advance(&sans, 'w', size) * 4.0;
    // Room for "wwww wwww" but not a third word.
    let width = word.mul_add(2.0, advance(&sans, ' ', size)) + 1.0;
    let laid = lay_out(&sans, &[], size, &white("wwww wwww wwww"), (Wrap::At { width }, Align::Left));

    assert_eq!(laid.lines, 2);
    let lines = baselines(&laid);
    assert_eq!(lines.len(), 2);
    let (first, second) = (line_at(&laid, lines[0]), line_at(&laid, lines[1]));
    assert_eq!((first.len(), second.len()), (8, 4), "two words, then one");
    assert!(first[0].abs() < 0.01 && second[0].abs() < 0.01, "each line starts at the left: {first:?} {second:?}");
    let line_height = lines[1] - lines[0];
    // A line's height is some thousands of font units, which an f32 holds exactly.
    #[allow(clippy::cast_precision_loss)]
    let expected = sans.font.line_metrics().line_height() as f32 * sans.font.pixels_per_unit(size);
    assert!((line_height - expected).abs() < 0.01, "lines are a line height apart: {line_height} {expected}");
}

#[test]
fn a_word_wider_than_the_wrap_is_left_whole() {
    // Catches a wrap that breaks a line with no space in it, or loops on it.
    let (sans, _) = faces();
    let laid = lay_out(&sans, &[], 20.0, &white("wwwwwwwwww"), (Wrap::At { width: 10.0 }, Align::Left));

    assert_eq!((laid.lines, laid.glyphs.len()), (1, 10));
}

#[test]
fn right_and_centred_lines_end_at_and_straddle_the_wrap_width_whatever_their_trailing_spaces() {
    // Catches a line aligned by a width that counts the space it was broken after, which sets a right-aligned line a
    // space short of the edge, and alignment within the widest line where the wrap width was asked for.
    let (sans, _) = faces();
    let size = 16.0;
    let width = 200.0;
    let runs = white("ab ab ab ab ab ab ab ab ab ab ab ab");
    let right = lay_out(&sans, &[], size, &runs, (Wrap::At { width }, Align::Right));
    let centred = lay_out(&sans, &[], size, &runs, (Wrap::At { width }, Align::Centre));
    assert!(right.lines > 1, "the text wraps");
    // Every line ends in a `b`, so it ends where its last glyph's origin is, and one `b` further.
    let ends = |laid: &Laid, baseline: f32| {
        let line = line_at(laid, baseline);
        let first = line.first().copied().unwrap_or(0.0);
        let last = line.last().copied().unwrap_or(0.0);
        (first, last + advance(&sans, 'b', size))
    };

    for baseline in baselines(&right) {
        let (_, end) = ends(&right, baseline);
        assert!((end - width).abs() < 0.01, "a right-aligned line ends at {end}, not {width}");
    }
    for baseline in baselines(&centred) {
        let (start, end) = ends(&centred, baseline);
        let middle = f32::midpoint(start, end);
        assert!((middle - width / 2.0).abs() < 0.01, "a centred line's middle is {middle}, not {}", width / 2.0);
    }
}

#[test]
fn a_character_the_font_lacks_is_drawn_by_the_first_fallback_that_has_it_and_else_as_the_missing_glyph() {
    // Catches a fallback passed over, a fallback glyph set by the text's own font's advance or kerned against its
    // neighbours in another font's units, and a character no font has listed once for each time it appears.
    let (sans, mono) = faces();
    assert!(sans.font.glyph(ONLY_MONO).is_none() && mono.font.glyph(ONLY_MONO).is_some(), "the fixture's premise");
    let size = 30.0;
    let text = format!("a{ONLY_MONO}b{IN_NEITHER}{IN_NEITHER}");
    let laid = lay_out(&sans, slice::from_ref(&mono), size, &white(&text), (Wrap::None, Align::Left));

    let fonts: Vec<FontKey> = laid.glyphs.iter().map(|glyph| glyph.key.font).collect();
    assert_eq!(fonts, [FontKey::Sans, FontKey::Made(7), FontKey::Sans, FontKey::Sans, FontKey::Sans]);
    assert_eq!(laid.glyphs[3].key.glyph, GlyphId::MISSING);
    assert_eq!(laid.missing, vec![IN_NEITHER], "listed once");
    let expected = advance(&sans, 'a', size) + advance(&mono, ONLY_MONO, size);
    assert!((laid.glyphs[2].origin.x - expected).abs() < 0.01, "b follows the line by its own font's advance");

    let alone = lay_out(&sans, &[], size, &white(&text), (Wrap::None, Align::Left));
    assert_eq!(alone.glyphs[1].key, GlyphKey { font: FontKey::Sans, glyph: GlyphId::MISSING });
    assert_eq!(alone.missing, vec![ONLY_MONO, IN_NEITHER]);
}

#[test]
fn each_glyph_takes_its_runs_colour() {
    // Catches runs' colours given by the glyph's index rather than its place in the text, which goes wrong once a run
    // holds a space or a newline that draws no glyph.
    let (sans, _) = faces();
    let red = Colour::opaque(1.0, 0.0, 0.0);
    let runs = vec![Run::new("a b\n", Colour::WHITE), Run::new("cd", red)];
    let laid = lay_out(&sans, &[], 12.0, &runs, (Wrap::None, Align::Left));

    let colours: Vec<Colour> = laid.glyphs.iter().map(|glyph| glyph.colour).collect();
    assert_eq!(colours, [Colour::WHITE, Colour::WHITE, red, red]);
    assert_eq!(laid.lines, 2);
}
