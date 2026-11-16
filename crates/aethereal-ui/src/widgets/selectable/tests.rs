//! `Selectable`: where a press lands in its text as drawn, what a double and a triple click select, and what it copies.

use super::{Selectable, Selecting};
use crate::fonts::{Fonts, Texts};
use crate::input::{Clip, Held, Input, Key, Pointing};
use crate::style::Style;
use crate::widget::{Laid, Span, Widget};
use aethereal_math::{Rect2, Vec2};
use aethereal_render::font::Font as FontFile;
use aethereal_render::text::{Align, Font, Wrap};
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

/// Presses on `text` styled `style`, laid in a box 400 by 200, at each of `presses`: where, and how many in a row; then
/// presses Control with C. What it selected and what it asked the clipboard.
fn pressed(text: &str, style: &Style, presses: &[(Vec2, u32)]) -> (Selecting, Vec<Clip>) {
    let fonts = fonts();
    let mut texts = Texts::default();
    let mut measuring = fonts.measuring(&mut texts, 1.0);
    let rect = Rect2::from_origin_size(Vec2::ZERO, Vec2::new(400.0, 200.0));
    let laid = Laid { rect, content: rect, clip: rect, extent: Vec2::ZERO, bar: Span::WHOLE, scale: 1.0 };
    let selectable = Selectable::new(text);
    let mut selecting = Selecting::default();
    let (mut raised, mut clips) = (Vec::new(), Vec::new());
    let mut input = Input::new(style, laid, &mut measuring, (&mut raised, &mut clips));
    for &(at, presses) in presses {
        selectable.pointed(&mut selecting, Pointing::Down { at, button: Button::Main, presses }, &mut input);
        selectable.pointed(&mut selecting, Pointing::Up { at, button: Button::Main, over: true }, &mut input);
    }
    let held = Held { control: true, ..Held::default() };
    let copy = Key { code: String::from("KeyC"), key: String::from("c"), repeat: false, held };
    selectable.key(&mut selecting, &copy, &mut input);

    (selecting, clips)
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

/// The fonts a cell is measured in: the monospace one alone.
fn fonts() -> Fonts {
    let mut fonts = Fonts::default();
    let Ok(mono) = FontFile::read(MONO) else {
        panic!("the renderer's monospace font reads");
    };
    fonts.read(Font::Mono, mono.metrics().clone(), Vec::new());

    fonts
}
