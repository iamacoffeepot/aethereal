//! `Selectable`: where a press lands in its text as drawn, what a double and a triple click select, what it copies, and
//! how a text of several colours draws beneath and over its selection.

use super::{SELECTION_STRENGTH, Selectable, Selecting};
use crate::fonts::{Fonts, Texts};
use crate::input::{Clip, Held, Input, Key, Pointing};
use crate::style::Style;
use crate::widget::{Draws, Focused, Laid, Lit, Piece, Span, Widget};
use aethereal_math::{Rect2, Vec2};
use aethereal_render::font::Font as FontFile;
use aethereal_render::text::{Align, Colour, Font, Run, Wrap};
use aethereal_window::Button;

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

/// `text`, in one run of white.
fn plain(text: &str) -> Selectable {
    Selectable { runs: vec![Run::new(text, Colour::WHITE)] }
}

/// Puts `pointings` to `selectable` styled `style`, laid in [`laid`]'s box, then presses Control with C. What it
/// selected and what it asked the clipboard.
fn pointed(selectable: &Selectable, style: &Style, pointings: &[Pointing]) -> (Selecting, Vec<Clip>) {
    let fonts = fonts();
    let mut texts = Texts::default();
    let mut measuring = fonts.measuring(&mut texts, 1.0);
    let mut selecting = Selecting::default();
    let (mut raised, mut clips) = (Vec::new(), Vec::new());
    let mut input = Input::new(style, laid(), &mut measuring, (&mut raised, &mut clips));
    for &pointing in pointings {
        selectable.pointed(&mut selecting, pointing, &mut input);
    }
    let held = Held { control: true, ..Held::default() };
    let copy = Key { code: String::from("KeyC"), key: String::from("c"), repeat: false, held };
    selectable.key(&mut selecting, &copy, &mut input);

    (selecting, clips)
}

/// Presses on `text` styled `style` at each of `presses`, where and how many in a row, then copies: what it selected
/// and what it asked the clipboard.
fn pressed(text: &str, style: &Style, presses: &[(Vec2, u32)]) -> (Selecting, Vec<Clip>) {
    let pointings: Vec<Pointing> = presses
        .iter()
        .flat_map(|&(at, presses)| {
            [
                Pointing::Down { at, button: Button::Main, presses },
                Pointing::Up { at, button: Button::Main, over: true },
            ]
        })
        .collect();

    pointed(&plain(text), style, &pointings)
}

/// The monospace style, aligned as `align` says.
fn style(align: Align) -> Style {
    Style { font: Font::Mono, font_size: SIZE, text_align: align, ..Style::default() }
}

#[test]
fn a_double_click_on_centred_text_selects_the_word_drawn_under_it_and_copies_it() {
    // Catches a press read as though the text stood at its box's left edge, when its alignment sets it further in, so
    // the word selected is not the one under the pointer; and a copy of the whole text rather than the selection.
    let (width, height) = cell(&fonts());
    let text = "ab cd";
    let left = width.mul_add(-5.0, 400.0) / 2.0;
    let on_cd = Vec2::new(width.mul_add(3.5, left), height / 2.0);

    let (selecting, clips) = pressed(text, &style(Align::Centre), &[(on_cd, 1), (on_cd, 2)]);

    assert_eq!((selecting.anchor, selecting.caret), (3, 5));
    assert_eq!(clips, vec![Clip::Copy(String::from("cd"))]);
}

#[test]
fn a_triple_click_selects_the_line_between_breaks() {
    // Catches a triple click that selects only the word, or the whole text across its breaks.
    let (width, height) = cell(&fonts());
    let on_three = Vec2::new(width * 2.5, height * 1.5);

    let (selecting, clips) = pressed("one two\nthree\nfour", &style(Align::Left), &[(on_three, 3)]);

    assert_eq!((selecting.anchor, selecting.caret), (8, 13));
    assert_eq!(clips, vec![Clip::Copy(String::from("three"))]);
}

#[test]
fn a_drag_across_runs_of_several_colours_copies_their_plain_text_and_draws_each_run_in_its_own_over_the_selection() {
    // Catches a selection's offsets counted in one run rather than the runs joined, so a drag past the first run's end
    // selects or copies the wrong characters; runs drawn in the style's one colour, or merged, losing their own; and a
    // selection drawn over its text, hiding it, or not at all while the text has its screen's focus.
    let fonts = fonts();
    let (width, height) = cell(&fonts);
    let (red, green) = (Colour::opaque(1.0, 0.0, 0.0), Colour::opaque(0.0, 1.0, 0.0));
    let runs = vec![Run::new("local", red), Run::new(" > ", green), Run::new("memory", Colour::WHITE)];
    let selectable = Selectable { runs: runs.clone() };
    let highlight = Colour::opaque(0.2, 0.4, 0.8);
    let style = Style { colour: highlight, ..style(Align::Left) };
    let (start, end) = (Vec2::new(0.0, height / 2.0), Vec2::new(width * 9.0, height / 2.0));
    let drag = [
        Pointing::Down { at: start, button: Button::Main, presses: 1 },
        Pointing::Moved { at: end },
        Pointing::Up { at: end, button: Button::Main, over: true },
    ];

    let (selecting, clips) = pointed(&selectable, &style, &drag);
    let mut texts = Texts::default();
    let mut measuring = fonts.measuring(&mut texts, 1.0);
    let mut pieces = Vec::new();
    let mut draws = Draws::new(&mut pieces, &mut measuring, Focused::Yes, Lit::No);
    Widget::draw(&selectable, &selecting, &style, &laid(), &mut draws);

    assert_eq!(clips, vec![Clip::Copy(String::from("local > m"))]);
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
