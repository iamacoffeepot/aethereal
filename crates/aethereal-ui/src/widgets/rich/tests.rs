//! `Rich`: a press and release on a link raise its target, found where the text was laid out to be drawn; and an
//! unbroken text keeps its lines whatever width it is offered.

use super::{Flow, Link, Rich};
use crate::fonts::{Fonts, Texts};
use crate::input::{Input, Pointing, Raised, Raises};
use crate::style::Style;
use crate::widget::{Laid, Offer, Offered, Span, Widget};
use aethereal_math::{Rect2, Vec2};
use aethereal_render::font::{Font as FontFile, Metrics};
use aethereal_render::text::{Colour, Font, Run, Wrap};
use aethereal_window::Button;

/// The renderer's own proportional font.
const SANS: &[u8] = include_bytes!("../../../../aethereal-render/fonts/IBMPlexSans-Regular.ttf");

/// The tests' screen event: a link pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ev {
    Open,
}

/// The proportional font's metrics, as the renderer hands them.
fn sans() -> Metrics {
    let Ok(font) = FontFile::read(SANS) else {
        panic!("the renderer's proportional font reads");
    };

    font.metrics().clone()
}

/// "Read the " then a link to `https://example.com`, "docs", then " now".
fn linked() -> Rich<Ev> {
    let colour = Colour::WHITE;
    Rich {
        runs: vec![Run::new("Read the ", colour), Run::new("docs", colour), Run::new(" now", colour)],
        links: vec![Link { run: 1, target: String::from("https://example.com") }],
        flow: Flow::Wraps,
        link: Raises::Event(Ev::Open),
    }
}

/// What a press and a release at `at`, in the text's box 400 by 40 at the screen's top left, raise.
fn clicked(rich: &Rich<Ev>, at: Vec2) -> Vec<Raised<Ev>> {
    let mut fonts = Fonts::default();
    fonts.read(Font::Sans, sans(), Vec::new());
    let mut texts = Texts::default();
    let mut measuring = fonts.measuring(&mut texts, 1.0);
    let style = Style::default();
    let rect = Rect2::from_origin_size(Vec2::ZERO, Vec2::new(400.0, 40.0));
    let laid = Laid { rect, content: rect, clip: rect, extent: Vec2::ZERO, bar: Span::WHOLE, scale: 1.0 };
    let mut raised = Vec::new();
    let mut state = super::Reading::default();
    for pointing in [Pointing::Down { at, button: Button::Main }, Pointing::Up { at, button: Button::Main, over: true }]
    {
        let mut input = Input::new(&style, laid, &mut measuring, &mut raised);
        Widget::pointed(rich, &mut state, pointing, &mut input);
    }

    raised.into_iter().filter_map(|raised| raised.typed::<Ev>().ok()).collect()
}

/// How wide `text` is in the proportional font at the default size.
fn width_of(text: &str) -> f32 {
    let mut fonts = Fonts::default();
    fonts.read(Font::Sans, sans(), Vec::new());
    let mut texts = Texts::default();
    let mut measuring = fonts.measuring(&mut texts, 1.0);
    let Ok(measured) = measuring.text(Font::Sans, 16.0, &[Run::new(text, Colour::WHITE)], Wrap::None) else {
        panic!("the font's metrics are known");
    };

    measured.width
}

#[test]
fn a_click_on_a_link_raises_its_target_and_one_beside_it_raises_nothing() {
    // Catches the run under the pointer found by a layout other than the one drawn, which shifts every link by the
    // runs before it, and a click on plain text taken for the link beside it.
    let rich = linked();
    let into_docs = width_of("Read the ") + width_of("do");
    let on_read = width_of("Re");

    assert_eq!(
        clicked(&rich, Vec2::new(into_docs, 10.0)),
        vec![Raised::Text(Ev::Open, String::from("https://example.com"))]
    );
    assert_eq!(clicked(&rich, Vec2::new(on_read, 10.0)), Vec::new());
}

#[test]
fn an_unbroken_text_measures_one_line_however_narrow_it_is_offered() {
    // Catches code wrapped to the column it sits in, where it scrolls across instead.
    let mut fonts = Fonts::default();
    fonts.read(Font::Sans, sans(), Vec::new());
    let mut texts = Texts::default();
    let mut measuring = fonts.measuring(&mut texts, 1.0);
    let style = Style::default();
    let code = Rich { flow: Flow::Unbroken, ..linked() };
    let narrow = Offer { width: Offered::AtMost(20.0), height: Offered::MaxContent };

    let measured = Widget::measure(&code, &style, &mut measuring, narrow);
    let line = Widget::least(&code, &style, &mut measuring);

    assert!(measured.x > 20.0, "it keeps its width: {measured:?}");
    assert_eq!(measured.y, line.y);
}
