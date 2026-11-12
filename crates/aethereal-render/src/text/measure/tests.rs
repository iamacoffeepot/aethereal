//! Measuring texts in the renderer's own fonts and the fixture font: the box each is laid out in, by its glyphs'
//! advances and kerning, its fallbacks in order, its wrapping, and its own font's line height.

use super::*;
use crate::font::{Font, GlyphId};
use crate::text::Colour;
use crate::text::faces::{MONO, SANS};

/// Noto Sans Regular (SIL OFL), the font module's fixture: its lines are spaced unlike the renderer's own fonts.
const NOTO: &[u8] = include_bytes!("../../../fixtures/fonts/NotoSans-Regular.ttf");

/// A character the renderer's own monospace font draws and its proportional one and Noto lack: a box-drawing line.
const ONLY_MONO: char = '\u{2500}';

/// A character Noto draws and the renderer's own fonts lack: a Latin b with a stroke.
const ONLY_NOTO: char = '\u{180}';

/// A character both the renderer's own fonts draw, at different widths, and Noto lacks: an arrow.
const NOT_NOTO: char = '\u{2192}';

/// A character no font here draws: an emoji.
const IN_NONE: char = '\u{1f600}';

/// No fallbacks.
const NO_FALLBACKS: &[Metrics] = &[];

/// The metrics of the font of `bytes`.
fn metrics(bytes: &[u8]) -> Metrics {
    let Ok(font) = Font::read(bytes) else {
        panic!("the font reads");
    };

    font.metrics().clone()
}

/// One white run of `text`.
fn white(text: &str) -> Vec<Run> {
    vec![Run::new(text, Colour::WHITE)]
}

/// How far `character`'s glyph, or the missing glyph when the font lacks it, moves the pen in the font of `metrics`
/// at `size`, in pixels.
fn advance(metrics: &Metrics, character: char, size: f32) -> f32 {
    let glyph = metrics.glyph(character).unwrap_or(GlyphId::MISSING);

    f32::from(metrics.advance(glyph).unwrap_or(0)) * metrics.pixels_per_unit(size)
}

/// How far apart the font of `metrics` sets its lines at `size`, in pixels.
fn line_height(metrics: &Metrics, size: f32) -> f32 {
    // A line's height is some thousands of font units, which an f32 holds exactly.
    #[allow(clippy::cast_precision_loss)]
    let units = metrics.line_metrics().line_height() as f32;

    units * metrics.pixels_per_unit(size)
}

/// Whether `measured` is `width` wide, `lines` lines, and `lines` times `line` tall, to a hundredth of a pixel.
fn measures(measured: Measured, width: f32, (lines, line): (u32, f32)) -> bool {
    // A count of lines is a few, which an f32 holds exactly.
    #[allow(clippy::cast_precision_loss)]
    let height = line * lines as f32;
    let wide = (measured.width - width).abs() < 0.01;
    let tall = (measured.height - height).abs() < 0.01;

    wide && tall && measured.lines == lines
}

#[test]
fn a_line_measures_its_glyphs_advances_kerned_and_not_its_trailing_space() {
    // Catches a measure that sums advances without the font's kerning, counts the space a line ends in, which draws
    // nothing, or is not a line height tall: a label sized by it would be cut, or sit off its box.
    let sans = metrics(SANS);
    let size = 32.0;
    let (a, v) = (sans.glyph('A'), sans.glyph('V'));
    let kerned = a.zip(v).map_or(0, |(a, v)| sans.kerning(a, v));
    assert!(kerned < 0, "the fixture's premise: A and V are kerned together");

    let measured = measure(&sans, NO_FALLBACKS, size, &white("AV "), Wrap::None);

    // A kerning is some tens of font units, which an f32 holds exactly.
    #[allow(clippy::cast_precision_loss)]
    let width =
        (kerned as f32).mul_add(sans.pixels_per_unit(size), advance(&sans, 'A', size) + advance(&sans, 'V', size));
    assert!(measures(measured, width, (1, line_height(&sans, size))), "{measured:?} is not {width} by one line");
}

#[test]
fn a_character_a_fallback_has_measures_as_it_is_there_and_one_no_font_has_as_the_missing_glyph() {
    // Catches a fallback's character measured by its own font's missing glyph, a character no font has measured as
    // nothing, and a text measured as tall as a fallback's lines: the renderer draws each at its own advance, in its
    // own font's lines.
    let (sans, mono, noto) = (metrics(SANS), metrics(MONO), metrics(NOTO));
    let size = 24.0;
    let premise = sans.glyph(ONLY_MONO).is_none() && sans.glyph(ONLY_NOTO).is_none() && mono.glyph(ONLY_NOTO).is_none();
    assert!(premise, "the fixture's premise: the box line is Mono's and the stroked b Noto's alone");
    assert!((line_height(&sans, size) - line_height(&noto, size)).abs() > 1.0, "Noto's lines are spaced apart");
    let text = format!("a{ONLY_MONO}b{ONLY_NOTO}{IN_NONE}");

    let measured = measure(&sans, &[mono.clone(), noto.clone()], size, &white(&text), Wrap::None);

    let width = advance(&sans, 'a', size)
        + advance(&mono, ONLY_MONO, size)
        + advance(&sans, 'b', size)
        + advance(&noto, ONLY_NOTO, size)
        + advance(&sans, IN_NONE, size);
    assert!(measures(measured, width, (1, line_height(&sans, size))), "{measured:?} is not {width} by one line");
}

#[test]
fn fallbacks_are_tried_in_the_order_they_are_listed() {
    // Catches fallbacks searched in another order than listed, or the last that has a character taken: the arrow
    // would measure at the other font's width.
    let (sans, mono, noto) = (metrics(SANS), metrics(MONO), metrics(NOTO));
    let size = 20.0;
    let premise = noto.glyph(NOT_NOTO).is_none() && sans.glyph(NOT_NOTO).is_some() && mono.glyph(NOT_NOTO).is_some();
    assert!(premise, "the fixture's premise: both of the renderer's own fonts draw the arrow, and Noto not");
    let arrows = white(&NOT_NOTO.to_string().repeat(3));

    let sans_first = measure(&noto, &[sans.clone(), mono.clone()], size, &arrows, Wrap::None);
    let mono_first = measure(&noto, &[mono.clone(), sans.clone()], size, &arrows, Wrap::None);

    let (by_sans, by_mono) = (advance(&sans, NOT_NOTO, size) * 3.0, advance(&mono, NOT_NOTO, size) * 3.0);
    assert!((by_sans - by_mono).abs() > 1.0, "the fixture's premise: the arrows are of different widths");
    assert!(measures(sans_first, by_sans, (1, line_height(&noto, size))), "{sans_first:?}");
    assert!(measures(mono_first, by_mono, (1, line_height(&noto, size))), "{mono_first:?}");
}

#[test]
fn a_text_measures_its_widest_line_wrapped_or_not_and_wrapped_at_that_breaks_where_it_was_measured() {
    // Catches the wrap width given back for a wrapped text whose lines are all narrower, so a box sized by it never
    // shrinks to its text; lines counted before they are wrapped; and a measured width at which the text, made to be
    // drawn in it, breaks elsewhere than it was measured.
    let sans = metrics(SANS);
    let size = 20.0;
    let (w, space) = (advance(&sans, 'w', size), advance(&sans, ' ', size));
    let word = w * 4.0;
    let widest = word.mul_add(2.0, space);
    // Room for "wwww wwww" and one more `w`, so "wwww w wwww" breaks after its second word.
    let offered = widest + w;
    let runs = white("wwww w wwww wwww");
    let line = line_height(&sans, size);

    let wrapped = measure(&sans, NO_FALLBACKS, size, &runs, Wrap::At { width: offered });
    let unwrapped = measure(&sans, NO_FALLBACKS, size, &white("ww\nwwww\nw"), Wrap::None);

    assert!(measures(wrapped, widest, (2, line)), "{wrapped:?} is not {widest} by two lines");
    assert!(measures(unwrapped, word, (3, line)), "{unwrapped:?} is not {word} by three lines");
    let drawn = |width| lay_out(&sans, NO_FALLBACKS, size, &runs, (Wrap::At { width }, Align::Left)).glyphs;
    assert_eq!(drawn(wrapped.width), drawn(offered), "wrapped at its measured width, it breaks where it was measured");
}
