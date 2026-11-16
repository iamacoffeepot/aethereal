//! `TextInput`: where typed text and deletions land, how the caret moves by character, word, and line, how it wraps,
//! grows, and slides through its lines, how the pointer selects, and what Enter does.

use super::edit::{Edit, Selecting};
use super::{Editing, Enter, TextInput};
use crate::fonts::{Fonts, Texts};
use crate::input::{Held, Input, Key, Pointing, Raised, Used};
use crate::style::Style;
use crate::widget::{Draws, Focused, Laid, Lit, Offer, Offered, Piece, Span, Widget};
use aethereal_math::{Rect2, Vec2};
use aethereal_render::font::{Font as FontFile, Metrics};
use aethereal_render::text::Font;
use aethereal_window::Button;

/// The renderer's own monospace font, so each character is as wide as the next.
const MONO: &[u8] = include_bytes!("../../../../aethereal-render/fonts/IBMPlexMono-Regular.ttf");

/// The size the tests set their text at, in pixels to the em.
const SIZE: f32 = 20.0;

/// The events the tests' text inputs raise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ev {
    Changed,
    Submitted,
}

/// The monospace font's metrics, as the renderer hands them.
fn mono() -> Metrics {
    let Ok(font) = FontFile::read(MONO) else {
        panic!("the renderer's monospace font reads");
    };

    font.metrics().clone()
}

/// The fonts the tests measure with: the monospace one alone.
fn fonts() -> Fonts {
    let mut fonts = Fonts::default();
    fonts.read(Font::Mono, mono(), Vec::new());

    fonts
}

/// The style the tests' text inputs are set in: the monospace font at [`SIZE`].
fn style() -> Style {
    Style { font: Font::Mono, font_size: SIZE, ..Style::default() }
}

/// How wide one character of the monospace font is at [`SIZE`], and how tall a line.
fn cell() -> (f32, f32) {
    let metrics = mono();
    let Some(glyph) = metrics.glyph('m') else {
        panic!("the monospace font has an m");
    };
    let width = f32::from(metrics.advance(glyph).unwrap_or(0)) * metrics.pixels_per_unit(SIZE);
    // A line's height is some thousands of font units, which an f32 holds exactly.
    #[allow(clippy::cast_precision_loss)]
    let height = metrics.line_metrics().line_height() as f32 * metrics.pixels_per_unit(SIZE);

    (width, height)
}

/// A text input laid out `columns` characters wide and `rows` lines tall, at a UI scale of 1.
fn laid(columns: f32, rows: f32) -> Laid {
    let (width, height) = cell();
    let rect = Rect2::from_origin_size(Vec2::ZERO, Vec2::new(width.mul_add(columns, 0.5), height * rows));

    Laid { rect, content: rect, clip: rect, extent: Vec2::ZERO, bar: Span::WHOLE, scale: 1.0 }
}

/// A text input under test, and its state, which each input reaches as a screen would hand it.
struct Typing {
    input: TextInput<Ev>,
    editing: Editing,
    laid: Laid,
    raised: Vec<Raised<Ev>>,
}

impl Typing {
    /// `input`, laid out `columns` characters wide and `rows` lines tall, its state as first made.
    fn new(input: TextInput<Ev>, columns: f32, rows: f32) -> Self {
        Self { input, editing: Editing::default(), laid: laid(columns, rows), raised: Vec::new() }
    }

    /// Hands it what `take` does with it, and takes in each change it raised as its screen does, keeping the text.
    fn reach<T>(&mut self, take: impl FnOnce(&TextInput<Ev>, &mut Editing, &mut Input<'_, '_>) -> T) -> T {
        let fonts = fonts();
        let mut texts = Texts::default();
        let mut measuring = fonts.measuring(&mut texts, 1.0);
        let style = style();
        let mut raised = Vec::new();
        let mut input = Input::new(&style, self.laid, &mut measuring, &mut raised);
        let took = take(&self.input, &mut self.editing, &mut input);
        for event in raised.into_iter().filter_map(|event| event.typed::<Ev>().ok()) {
            if let Raised::Text(Ev::Changed, text) = &event {
                self.input.text.clone_from(text);
            }
            self.raised.push(event);
        }

        took
    }

    /// Presses the key at `code`, `held` held.
    fn key(&mut self, code: &str, held: Held) -> Used {
        let key = Key { code: String::from(code), key: String::from(code), repeat: false, held };
        self.reach(|input, editing, reached| input.key(editing, &key, reached))
    }

    /// Types `text`.
    fn typed(&mut self, text: &str) {
        self.reach(|input, editing, reached| input.typed(editing, text, reached));
    }

    /// Does `pointing` at it.
    fn point(&mut self, pointing: Pointing) {
        self.reach(|input, editing, reached| input.pointed(editing, pointing, reached));
    }

    /// Presses the main button at the `column`th character of the `row`th line shown, `presses` in a row, and lets go.
    fn click(&mut self, (column, row): (f32, f32), presses: u32) {
        let (width, height) = cell();
        let at = Vec2::new(width * column, height * (row + 0.5));
        self.point(Pointing::Down { at, button: Button::Main, presses });
        self.point(Pointing::Up { at, button: Button::Main, over: true });
    }

    /// Where its selection runs.
    fn selected(&self) -> (u32, u32) {
        (self.editing.anchor.min(self.editing.caret), self.editing.anchor.max(self.editing.caret))
    }

    /// The submits it raised, each with its text.
    fn submitted(&self) -> Vec<String> {
        let submits = self.raised.iter().filter_map(|raised| match raised {
            Raised::Text(Ev::Submitted, text) => Some(text.clone()),
            _ => None,
        });

        submits.collect()
    }
}

/// No modifier held.
const PLAIN: Held = Held { shift: false, control: false, alt: false, meta: false };

/// Shift held.
const SHIFT: Held = Held { shift: true, ..PLAIN };

#[test]
fn typed_text_goes_in_at_the_caret_and_replaces_the_selection() {
    // Catches text appended at the end whatever the caret, and typing over a selection that inserts beside it rather
    // than in its place, leaving the selected text behind.
    let mut at_caret = Edit::new("gold", 2, 2);
    at_caret.typed("XY");

    let mut over_selection = Edit::new("gold", 1, 3);
    over_selection.typed("e");

    assert_eq!(at_caret, Edit { text: String::from("goXYld"), caret: 4, anchor: 4 });
    assert_eq!(over_selection, Edit { text: String::from("ged"), caret: 2, anchor: 2 });
}

#[test]
fn the_caret_steps_over_whole_characters_and_backspace_deletes_one() {
    // Catches a caret moved a byte at a time, which lands inside a character of more than one byte and panics the next
    // edit there; and a Backspace that deletes a byte of one.
    let mut edit = Edit::new("añb", 4, 4);
    edit.move_to(edit.back(edit.caret), Selecting::Collapse);
    let stepped = edit.caret;
    edit.delete(edit.back(edit.caret));

    assert_eq!(stepped, 3, "back over the one-byte b");
    assert_eq!(edit, Edit { text: String::from("ab"), caret: 1, anchor: 1 });
}

#[test]
fn a_caret_inside_a_character_is_pulled_back_to_its_start() {
    // Catches a caret kept from a longer text, now inside a character of the text the screen shows, used as it is:
    // slicing there panics.
    let edit = Edit::new("ñ", 1, 9);

    assert_eq!((edit.caret, edit.anchor), (0, 2));
}

#[test]
fn a_move_by_word_stops_at_each_run_of_letters_or_marks_and_passes_spaces_and_breaks() {
    // Catches a word move that stops at every space, or at a line break, so it takes two presses to leave a line's end;
    // and one that treats punctuation as part of a word, so `foo.bar` is one jump where editors make it three.
    let edit = Edit::new("foo.bar  baz\nqux", 0, 0);
    let mut forward = vec![0];
    let mut at = 0;
    while at < edit.text.len() {
        at = edit.word_forward(at);
        forward.push(at);
    }
    let mut back = vec![edit.text.len()];
    while at > 0 {
        at = edit.word_back(at);
        back.push(at);
    }

    assert_eq!(forward, vec![0, 3, 4, 7, 12, 16]);
    assert_eq!(back, vec![16, 13, 9, 4, 3, 0]);
}

#[test]
fn it_wraps_and_grows_to_its_most_lines_then_slides_to_show_its_caret() {
    // Catches an input that stays one line tall however much it holds, one that grows past its most lines rather than
    // sliding, and one whose first line shown does not follow the caret down as it types, or up as it moves, so the
    // caret goes out of sight.
    let input = TextInput::new("", Ev::Changed).most_lines(3);
    let (width, line) = cell();
    let fonts = fonts();
    let mut texts = Texts::default();
    let mut measuring = fonts.measuring(&mut texts, 1.0);
    let offer = Offer { width: Offered::Exactly(width * 10.5), height: Offered::MaxContent };
    let tall = |text: &str, measuring: &mut _| {
        let input = TextInput { text: String::from(text), ..input.clone() };
        (Widget::measure(&input, &style(), measuring, offer).y / line).round()
    };
    let heights = ["one", "one two three", "one two three four five", "one two three four five six seven eight"]
        .map(|text| tall(text, &mut measuring));
    let mut typing = Typing::new(input, 10.0, 3.0);
    typing.typed("1\n2\n3\n4\n5");
    let typed_top = typing.editing.top;
    for _ in 0..4 {
        typing.key("ArrowUp", PLAIN);
    }

    assert_eq!(heights, [1.0, 2.0, 3.0, 3.0]);
    assert_eq!(typed_top, 2, "the caret on the fifth line shows the third to the fifth");
    assert_eq!((typing.editing.caret, typing.editing.top), (1, 0), "up to the first line, shown again");
}

#[test]
fn up_and_down_keep_the_place_along_the_line_they_started_from() {
    // Catches a move down that takes the place the caret was pulled to on a short line, so passing through one leaves
    // the caret at its end on every line after; and a move up from the first line that does not pass the key on.
    let text = "abcdefgh\nab\nabcdefgh";
    let mut typing = Typing::new(TextInput::new(text, Ev::Changed).most_lines(3), 20.0, 3.0);
    typing.click((6.0, 0.0), 1);
    typing.key("ArrowDown", PLAIN);
    let short = typing.editing.caret;
    typing.key("ArrowDown", PLAIN);
    let long = typing.editing.caret;
    typing.key("ArrowUp", PLAIN);
    typing.key("ArrowUp", PLAIN);
    let back = typing.editing.caret;

    assert_eq!((short, long, back), (11, 18, 6));
    assert_eq!(typing.key("ArrowUp", PLAIN), Used::Passed, "the first line passes Up on");
}

#[test]
fn shift_with_a_move_selects_and_the_pointer_selects_by_drag_word_and_line() {
    // Catches a Shift move that drags the anchor with the caret, so nothing is selected; a double click that only puts
    // the caret, or selects past the word into its punctuation; a triple click that selects the wrapped line rather than
    // the line between breaks; and a drag that moves the anchor too.
    let text = "pick one.word here\nnext";
    let mut typing = Typing::new(TextInput::new(text, Ev::Changed).most_lines(4), 40.0, 4.0);
    typing.click((0.0, 0.0), 1);
    typing.key("ArrowRight", SHIFT);
    typing.key("ArrowRight", Held { control: true, ..SHIFT });
    let by_keys = typing.selected();
    typing.click((6.0, 0.0), 1);
    typing.click((6.0, 0.0), 2);
    let word = typing.selected();
    typing.click((6.0, 0.0), 3);
    let line = typing.selected();
    let (width, height) = cell();
    typing.point(Pointing::Down { at: Vec2::new(width * 5.0, height * 0.5), button: Button::Main, presses: 1 });
    typing.point(Pointing::Moved { at: Vec2::new(width * 2.0, height * 1.5) });
    let dragged = typing.selected();

    assert_eq!(by_keys, (0, 4));
    assert_eq!(word, (5, 8), "one");
    assert_eq!(line, (0, 18));
    assert_eq!(dragged, (5, 21));
}

#[test]
fn enter_submits_and_shift_enter_breaks_the_line_or_the_other_way_round() {
    // Catches Shift-Enter submitting as Enter does, so a chat cannot write two lines; Enter typing a line break into
    // the text it submits; and an input set to break on Enter that still submits on it.
    let mut chat = Typing::new(TextInput::new("hi", Ev::Changed).submit(Ev::Submitted), 20.0, 3.0);
    chat.key("End", PLAIN);
    chat.key("Enter", SHIFT);
    chat.typed("there");
    chat.key("Enter", PLAIN);

    let mut editor =
        Typing::new(TextInput::new("hi", Ev::Changed).submit(Ev::Submitted).enter(Enter::Breaks), 20.0, 3.0);
    editor.key("Enter", PLAIN);
    editor.key("Enter", SHIFT);

    assert_eq!(chat.submitted(), vec![String::from("hi\nthere")]);
    assert_eq!(editor.submitted(), vec![String::from("hi\n")]);
}

#[test]
fn what_the_input_method_composes_is_drawn_at_the_caret_until_it_closes() {
    // Catches a composition drawn at the text's end whatever the caret, or kept once the input method closed it, so the
    // committed text then shows beside it twice.
    let mut typing = Typing::new(TextInput::new("abcd", Ev::Changed), 20.0, 1.0);
    typing.click((2.0, 0.0), 1);
    typing.reach(|input, editing, reached| input.composing(editing, "にほ", 3, reached));
    let open = drawn(&typing);
    typing.reach(|input, editing, reached| input.composing(editing, "", 0, reached));
    typing.typed("日本");

    assert_eq!(open, "abにほcd");
    assert_eq!(drawn(&typing), "ab日本cd");
}

/// The text `typing` draws.
fn drawn(typing: &Typing) -> String {
    let fonts = fonts();
    let mut texts = Texts::default();
    let mut measuring = fonts.measuring(&mut texts, 1.0);
    let mut pieces = Vec::new();
    let mut draws = Draws::new(&mut pieces, &mut measuring, Focused::Yes, Lit::No);
    Widget::draw(&typing.input, &typing.editing, &style(), &typing.laid, &mut draws);
    let texts = pieces.iter().filter_map(|piece| match piece {
        Piece::Text(text) => Some(text.runs.iter().map(|run| run.text.as_str()).collect::<String>()),
        Piece::Quad(_) => None,
    });

    texts.collect()
}
