//! `Selectable`: where a place in its text lands as it is drawn, and how a text of several colours draws beneath and over
//! the part its screen's selection holds.

use super::{SELECTION_STRENGTH, Selectable};
use crate::fonts::{Fonts, Texts};
use crate::style::Style;
use crate::widget::{Draws, Focused, Laid, Lit, Piece, Selected, Span, Stateless, Widget};
use aethereal_math::{Rect2, Vec2};
use aethereal_render::font::Font as FontFile;
use aethereal_render::text::{Align, Colour, Font, Run, Wrap};

/// The renderer's own monospace font, so each character is as wide as the next.
const MONO: &[u8] = include_bytes!("../../../../aethereal-render/fonts/IBMPlexMono-Regular.ttf");

/// The size the tests set their text at, in pixels to the em.
const SIZE: f32 = 20.0;

/// How wide one character of the monospace font is at [`SIZE`], and how tall a line.
fn cell(fonts: &Fonts) -> (f32, f32) {
    let mut texts = Texts::default();
    let mut measuring = fonts.measuring(&mut texts, 1.0);
    let Ok(carets) = measuring.carets(Font::Mono, SIZE, "m", Wrap::None) else {
        panic!("the monospace font's metrics are read");
    };

    (carets.lines[0].stops[1].x, carets.line_height)
}

/// The box the tests lay their text in: 400 by 200 at the top left.
fn laid() -> Laid {
    let rect = Rect2::from_origin_size(Vec2::ZERO, Vec2::new(400.0, 200.0));

    Laid { rect, content: rect, clip: rect, extent: Vec2::ZERO, bar: Span::WHOLE, scale: 1.0 }
}

/// The monospace style, aligned as `align` says.
fn style(align: Align) -> Style {
    Style { font: Font::Mono, font_size: SIZE, text_align: align, ..Style::default() }
}

#[test]
fn a_place_on_centred_text_lands_at_the_boundary_drawn_under_it() {
    // Catches a place read as though the text stood at its box's left edge, when its alignment sets it further in, so
    // a press or a drag puts the caret characters away from the pointer.
    let fonts = fonts();
    let (width, height) = cell(&fonts);
    let selectable = Selectable { runs: vec![Run::new("ab cd", Colour::WHITE)] };
    let left = width.mul_add(-5.0, 400.0) / 2.0;
    let after_c = Vec2::new(width.mul_add(3.9, left), height / 2.0);
    let mut texts = Texts::default();
    let mut measuring = fonts.measuring(&mut texts, 1.0);

    let boundary = Widget::boundary(&selectable, after_c, &style(Align::Centre), &laid(), &mut measuring);

    assert_eq!(boundary, 4);
}

#[test]
fn the_part_selected_of_runs_of_several_colours_is_drawn_beneath_them_and_each_run_in_its_own() {
    // Catches a selected part's offsets counted in one run rather than the runs joined, so it ends at the wrong
    // character; runs drawn in the style's one colour, or merged, losing their own; and the selection drawn over its
    // text, hiding it, or in another colour than the one it is told.
    let fonts = fonts();
    let (width, _) = cell(&fonts);
    let (red, green) = (Colour::opaque(1.0, 0.0, 0.0), Colour::opaque(0.0, 1.0, 0.0));
    let runs = vec![Run::new("local", red), Run::new(" > ", green), Run::new("memory", Colour::WHITE)];
    let selectable = Selectable { runs: runs.clone() };
    let highlight = Colour::opaque(0.2, 0.4, 0.8);
    let mut texts = Texts::default();
    let mut measuring = fonts.measuring(&mut texts, 1.0);
    let mut pieces = Vec::new();
    let selected = Selected::Part { start: 0, end: 9, colour: highlight };
    let mut draws = Draws::new(&mut pieces, &mut measuring, Focused::No, Lit::No).selecting(selected);

    Widget::draw(&selectable, &Stateless, &style(Align::Left), &laid(), &mut draws);

    let [Piece::Quad(selection), Piece::Text(written)] = pieces.as_slice() else {
        panic!("its selection, then its text: {pieces:?}");
    };
    assert_eq!(selection.colour, Colour { alpha: SELECTION_STRENGTH, ..highlight });
    assert!(width.mul_add(-9.0, selection.rect.max.x).abs() < 0.01, "the selection ends after `m`: {selection:?}");
    assert_eq!(written.runs, runs);
}

/// The fonts a cell is measured in: the monospace one alone.
fn fonts() -> Fonts {
    let mut fonts = Fonts::default();
    let Ok(mono) = FontFile::read(MONO) else {
        panic!("the renderer's monospace font reads");
    };
    fonts.read(Font::Mono, mono.metrics().clone(), Vec::new());

    fonts
}
