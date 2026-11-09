//! The checks a text is made by: its size, outline, wrap, runs, and font, each refused at its bound.

use super::*;

/// A text of `size` pixels to the em in the renderer's own proportional font, of `runs`, wrapped and outlined so.
fn asked(size: f32, runs: Vec<Run>, wrap: Wrap, outline: Outline) -> CreateText {
    CreateText { font: Font::Sans, size, runs, wrap, align: Align::Left, outline }
}

/// One white run of `text`.
fn white(text: &str) -> Vec<Run> {
    vec![Run::new(text, Colour::WHITE)]
}

#[test]
fn a_text_is_refused_outside_its_sizes_and_runs_and_with_no_font_and_made_at_their_edges() {
    // Catches a bound off by one, a size that is not a number let through, and runs counted in characters rather than
    // the bytes the limit is of.
    let plain = |size| asked(size, white("a"), Wrap::None, Outline::None);
    assert_eq!(check(&plain(1.0), true), Ok(()));
    assert_eq!(check(&plain(1024.0), true), Ok(()));
    for size in [0.99, 1024.5, f32::NAN, f32::INFINITY] {
        assert_eq!(check(&plain(size), true), Err(TextRefused::Size), "{size}");
    }
    assert_eq!(check(&plain(12.0), false), Err(TextRefused::Font));

    // Two bytes a character: half as many characters as bytes reach the limit.
    let half = "\u{e9}".repeat(32 * 1024);
    assert_eq!(check(&asked(12.0, white(&half), Wrap::None, Outline::None), true), Ok(()));
    let over = vec![Run::new(&half, Colour::WHITE), Run::new("a", Colour::WHITE)];
    let refused = Err(TextRefused::Runs { bytes: MOST_RUN_BYTES + 1 });
    assert_eq!(check(&asked(12.0, over, Wrap::None, Outline::None), true), refused);

    for width in [0.0, -1.0, f32::NAN] {
        assert_eq!(check(&asked(12.0, white("a"), Wrap::At { width }, Outline::None), true), Err(TextRefused::Wrap));
    }
}

#[test]
fn an_outline_is_refused_wider_than_its_fields_reach_at_the_text_s_size() {
    // Catches an outline measured against the base size the fields are generated at rather than the text's, which lets
    // a small text's rim run off the end of its field, cut square.
    let rimmed = |size, width| asked(size, white("a"), Wrap::None, Outline::Of { width, colour: Colour::BLACK });
    // A field reaches 4 pixels at 48 to the em: 2 pixels at 24, and 8 at 96.
    assert_eq!(check(&rimmed(24.0, 2.0), true), Ok(()));
    assert_eq!(check(&rimmed(24.0, 2.1), true), Err(TextRefused::Outline));
    assert_eq!(check(&rimmed(96.0, 8.0), true), Ok(()));
    assert_eq!(check(&rimmed(96.0, -0.5), true), Err(TextRefused::Outline));
}
