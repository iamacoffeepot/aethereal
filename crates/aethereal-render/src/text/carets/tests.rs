//! Carets in a text laid out as the renderer lays it out: which bytes each drawn line holds, and where each boundary
//! stands against the glyphs the renderer draws.

use super::*;
use crate::font::Font;
use crate::text::faces::SANS;
use crate::text::layout::lay_out;
use crate::text::{Align, Colour, Run};

/// No fallbacks.
const NO_FALLBACKS: &[Metrics] = &[];

/// The metrics of the renderer's own proportional font.
fn sans() -> Metrics {
    let Ok(sans) = Font::read(SANS) else {
        panic!("the renderer's own font is read");
    };

    sans.metrics().clone()
}

#[test]
fn a_wrapped_line_holds_the_space_it_broke_after_and_a_hard_line_ends_before_its_newline() {
    // Catches a line's bytes counted from the glyphs alone, which loses the newline between hard lines and starts the
    // line after an empty one at the wrong byte, and a wrapped line's end taken as the space before its break, which
    // would leave a caret at the break on neither line.
    let sans = sans();
    let text = "ab cd\n\nef";
    let two = lay_out(&sans, NO_FALLBACKS, 20.0, &[Run::new("ab", Colour::WHITE)], (Wrap::None, Align::Left)).width;

    let carets = carets(&sans, NO_FALLBACKS, 20.0, text, Wrap::At { width: two * 1.5 });
    let held: Vec<(usize, usize, Ends)> = carets.lines.iter().map(|line| (line.start, line.end, line.ends)).collect();

    assert_eq!(held, vec![(0, 3, Ends::Soft), (3, 5, Ends::Hard), (6, 6, Ends::Hard), (7, 9, Ends::Hard)]);
    assert_eq!(carets.line_of(3), 1, "a caret at the break stands at the next line's start");
    assert_eq!(carets.lines[0].last(), 2, "the first line's last caret is before the space it broke after");
}

#[test]
fn each_stop_stands_where_the_renderer_draws_its_glyph() {
    // Catches stops measured apart from the renderer's layout, without its kerning or its line's offset, so a caret
    // drawn between two kerned letters, or on a wrapped line, stands off the glyphs.
    let sans = sans();
    let text = "AVA To AVA";
    let wrap = Wrap::At { width: 60.0 };
    let drawn = lay_out(&sans, NO_FALLBACKS, 24.0, &[Run::new(text, Colour::WHITE)], (wrap, Align::Left));

    let carets = carets(&sans, NO_FALLBACKS, 24.0, text, wrap);
    let drawn_at = |stop: &&Stop| text[stop.at..].chars().next().is_some_and(|next| !next.is_whitespace());
    let glyphs_of = |line: &CaretLine| line.stops[..line.stops.len() - 1].to_vec();
    let lefts: Vec<f32> =
        carets.lines.iter().flat_map(glyphs_of).filter(|stop| drawn_at(&stop)).map(|stop| stop.x).collect();
    let origins: Vec<f32> = drawn.glyphs.iter().map(|glyph| glyph.origin.x).collect();

    assert!(carets.lines.len() > 1, "the text wraps");
    assert_eq!(lefts, origins);
}
