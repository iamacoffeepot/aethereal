//! `TextInput`: a line of text typed into (ADR-0056: Stock widgets; ADR-0051). Its text is the screen's: each edit
//! raises its `change` with the text as edited, which the screen's method keeps and the view shows, and Enter raises
//! its `submit` with the text. Its caret and selection are its own state, kept by its key.
//!
//! With focus it hears every key, blocking, and the window's input method is allowed: typed text goes in at the caret,
//! in place of the selection; Backspace and Delete delete; the arrows, Home, and End move the caret, with Shift taking
//! the selection with them; Control or Meta with A selects the whole. A key that types text is used, so a letter
//! typed never reaches the screen's hotkeys; a key it does not use, Tab or Escape or a function key, goes on to the
//! focus keys and the screen. A press puts the caret under the pointer, and a drag from it selects.
//!
//! A text its screen sets that is not its own last edit, a line recalled or completed, puts its caret at the text's
//! end.
//!
//! It draws its text on one line in its style's font, size, and colour, cut to its content box, and slid left so the
//! caret stays in it; with focus, its selection beneath the text and its caret over it. What an input method is still
//! composing is not shown: the window does not send it yet (ADR-0051: follow-on work).

mod line;
#[cfg(test)]
mod tests;

use line::{Line, Selecting, Toward};

use crate::fonts::{Measuring, Unmeasured};
use crate::input::{Event, Hears, Input, Key, Pointing, Raised, Raises, Raising, Used};
use crate::style::Style;
use crate::widget::{Arrange, Draws, Laid, Offer, Offered, Quad, Sample, TextDraw, Widget};
use aethereal_data::Plain;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::draw_list::Pointer;
use aethereal_render::text::{Align, Colour, Font, Outline, Run, Wrap};
use aethereal_window::{Button, Cursor};

/// How wide a text input is when nothing sizes it, in ems of its style's font.
const WIDTH_EMS: f32 = 10.0;

/// How wide its caret is, in logical pixels.
const CARET_WIDTH: f32 = 1.0;

/// How strong its selection is, of its style's colour's alpha.
const SELECTION_STRENGTH: f32 = 0.3;

/// A line of `text` typed into, raising `change` with each edit and, when it raises one, `submit` with Enter: markup's
/// `submit => …` sets it, and markup that leaves it out with `..` raises nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextInput<E> {
    pub text: String,
    pub change: E,
    pub submit: Raises<E>,
}

/// A text input's caret and the other end of its selection, byte offsets into its text; whether a press on it is
/// selecting as the pointer drags; and its text as it last edited it, which tells an edit of its own from a text its
/// screen set.
#[derive(Debug, Clone, PartialEq, Eq, Default, Plain)]
#[kind(name = "aether.ui.text_input")]
pub struct Editing {
    pub caret: u32,
    pub anchor: u32,
    pub dragging: bool,
    pub edited: String,
}

impl<E> TextInput<E> {
    /// A text input showing `text`, raising `change`, and no submit.
    #[must_use]
    pub fn new(text: &str, change: E) -> Self {
        Self { text: String::from(text), change, submit: Raises::Nothing }
    }

    /// It, raising `submit` with Enter.
    #[must_use]
    pub fn submit(self, submit: E) -> Self {
        Self { submit: Raises::Event(submit), ..self }
    }

    /// Its text as `editing` stands in it: a text its screen set, not one it last edited, with the caret at its end, as
    /// a line recalled from a history or completed is typed on from its end.
    fn line(&self, editing: &Editing) -> Line {
        let own = editing.edited == self.text;
        if !own {
            return Line::new(&self.text, self.text.len(), self.text.len());
        }

        Line::new(&self.text, offset(editing.caret), offset(editing.anchor))
    }
}

/// An offset kept in a state, as an offset into a text.
fn offset(kept: u32) -> usize {
    usize::try_from(kept).unwrap_or(usize::MAX)
}

/// An offset into a text, as a state keeps it: a text past four billion bytes keeps its caret at the most it counts.
fn kept(offset: usize) -> u32 {
    u32::try_from(offset).unwrap_or(u32::MAX)
}

/// Keeps `line`'s caret, anchor, and text in `editing`.
fn keep(line: &Line, editing: &mut Editing) {
    editing.caret = kept(line.caret);
    editing.anchor = kept(line.anchor);
    editing.edited.clone_from(&line.text);
}

/// How a text input's text is set, as its style and its screen's scale say.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Set {
    font: Font,
    size: f32,
    colour: Colour,
}

impl Set {
    /// The text of `style` at `scale`.
    fn of(style: &Style, scale: f32) -> Self {
        Self { font: style.font, size: style.font_size * scale, colour: style.colour }
    }

    /// How wide `text` is, on one line.
    fn width(self, measuring: &mut Measuring<'_>, text: &str) -> Result<f32, Unmeasured> {
        let runs = [Run::new(text, self.colour)];

        measuring.text(self.font, self.size, &runs, Wrap::None).map(|measured| measured.width)
    }

    /// How tall a line is.
    fn height(self, measuring: &mut Measuring<'_>) -> Result<f32, Unmeasured> {
        let runs = [Run::new(" ", self.colour)];

        measuring.text(self.font, self.size, &runs, Wrap::None).map(|measured| measured.height)
    }

    /// How far `line`'s text is slid left in a box `across` pixels wide so its caret shows: none while the caret is
    /// within the box from the text's start, and otherwise just enough that it is at the box's right edge.
    fn slid(self, measuring: &mut Measuring<'_>, line: &Line, across: f32, caret: f32) -> Result<f32, Unmeasured> {
        let to_caret = self.width(measuring, &line.text[..line.caret])?;

        Ok((to_caret + caret - across).max(0.0))
    }

    /// The offset of `line`'s text nearest `x` pixels from its start: each character's boundary measured from the
    /// start, the nearest taken.
    fn nearest(self, measuring: &mut Measuring<'_>, line: &Line, x: f32) -> Result<usize, Unmeasured> {
        let mut nearest = (0, f32::INFINITY);
        let ends = line.text.char_indices().map(|(at, _)| at).chain([line.text.len()]);
        for at in ends {
            let away = (self.width(measuring, &line.text[..at])? - x).abs();
            if away < nearest.1 {
                nearest = (at, away);
            }
        }

        Ok(nearest.0)
    }
}

/// Puts `line`'s caret under the pointer at `at`, in its input's own coordinates, as it is drawn: slid as its caret last
/// showed it.
fn caret_at(line: &mut Line, at: Vec2, input: &mut Input<'_, '_>) {
    let set = Set::of(input.style(), input.laid().scale);
    let content = input.laid().content;
    let caret = CARET_WIDTH * input.laid().scale;
    let measuring = input.measuring();
    let Ok(slid) = set.slid(measuring, line, content.width(), caret) else {
        return;
    };
    let Ok(nearest) = set.nearest(measuring, line, at.x - content.min.x + slid) else {
        return;
    };
    line.caret = nearest;
}

impl<E: Event> TextInput<E> {
    /// Raises `change` with `line`'s text, when an edit changed it.
    fn changed(&self, line: &Line, input: &mut Input<'_, '_>) {
        let edited = line.text != self.text;
        if edited {
            input.raise(Raised::Text(self.change, line.text.clone()));
        }
    }
}

impl<E: Event> Raising for TextInput<E> {
    type Event = E;
}

impl<E: Event> Widget for TextInput<E> {
    type State = Editing;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    /// The width it is offered, or ten ems with none, and one line down.
    fn measure(&self, style: &Style, measuring: &mut Measuring<'_>, offer: Offer) -> Vec2 {
        let set = Set::of(style, measuring.scale());
        let width = match offer.width {
            Offered::Exactly(width) | Offered::AtMost(width) => width,
            Offered::MinContent | Offered::MaxContent => set.size * WIDTH_EMS,
        };
        let height = set.height(measuring).unwrap_or(0.0);

        Vec2::new(width, height)
    }

    fn draw(&self, editing: &Editing, style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        let focused = draws.focused();
        let set = Set::of(style, laid.scale);
        let line = self.line(editing);
        let content = laid.content;
        let clip = laid.clip.intersection(content);
        let caret = CARET_WIDTH * laid.scale;
        let measuring = draws.measuring();
        let Ok(height) = set.height(measuring) else {
            return;
        };
        let slid = if focused {
            set.slid(measuring, &line, content.width(), caret).unwrap_or(0.0)
        } else {
            0.0
        };
        let top = content.min.y + (content.height() - height) / 2.0;
        let x = |measuring: &mut Measuring<'_>, at: usize| {
            set.width(measuring, &line.text[..at]).map(|width| content.min.x + width - slid)
        };
        let (start, end) = line.selected();
        let selecting = focused && start != end;
        if selecting {
            let (Ok(left), Ok(right)) = (x(measuring, start), x(measuring, end)) else {
                return;
            };
            let colour = Colour { alpha: style.colour.alpha * SELECTION_STRENGTH, ..style.colour };
            draws.quad(bar(Rect2 { min: Vec2::new(left, top), max: Vec2::new(right, top + height) }, clip, colour));
        }
        draws.text(TextDraw {
            at: Vec2::new(content.min.x - slid, top),
            clip,
            font: set.font,
            size: set.size,
            wrap: Wrap::None,
            align: Align::Left,
            outline: Outline::None,
            runs: vec![Run::new(&line.text, set.colour)],
        });
        if focused {
            let Ok(left) = x(draws.measuring(), line.caret) else {
                return;
            };
            let rect = Rect2::from_origin_size(Vec2::new(left, top), Vec2::new(caret, height));
            draws.quad(bar(rect, clip, set.colour));
        }
    }

    fn pointer(&self, _: &Editing) -> Pointer {
        Pointer::Blocks(Cursor::Text)
    }

    fn focusable(&self) -> bool {
        true
    }

    fn hears(&self) -> Hears {
        Hears::Typing
    }

    fn pointed(&self, editing: &mut Editing, pointing: Pointing, input: &mut Input<'_, '_>) {
        let mut line = self.line(editing);
        match pointing {
            Pointing::Down { at, button: Button::Main } => {
                caret_at(&mut line, at, input);
                line.anchor = line.caret;
                editing.dragging = true;
            }
            Pointing::Moved { at } if editing.dragging => caret_at(&mut line, at, input),
            Pointing::Up { button: Button::Main, .. } => editing.dragging = false,
            Pointing::Entered
            | Pointing::Left
            | Pointing::Moved { .. }
            | Pointing::Down { .. }
            | Pointing::Up { .. } => {}
        }
        keep(&line, editing);
    }

    fn key(&self, editing: &mut Editing, key: &Key, input: &mut Input<'_, '_>) -> Used {
        let mut line = self.line(editing);
        let selecting = if key.held.shift {
            Selecting::Extend
        } else {
            Selecting::Collapse
        };
        let all = key.code == "KeyA" && key.held.command;
        match key.code.as_str() {
            _ if all => line.all(),
            "Backspace" => line.delete(Toward::Back),
            "Delete" => line.delete(Toward::Forward),
            "ArrowLeft" => line.moved(Toward::Back, selecting),
            "ArrowRight" => line.moved(Toward::Forward, selecting),
            "Home" => line.moved(Toward::Start, selecting),
            "End" => line.moved(Toward::End, selecting),
            "Enter" | "NumpadEnter" => {
                if let Raises::Event(submit) = self.submit {
                    input.raise(Raised::Text(submit, self.text.clone()));
                }
            }
            _ if key.types() => {}
            _ => return Used::Passed,
        }
        self.changed(&line, input);
        keep(&line, editing);

        Used::Used
    }

    fn typed(&self, editing: &mut Editing, text: &str, input: &mut Input<'_, '_>) {
        let mut line = self.line(editing);
        line.typed(text);
        self.changed(&line, input);
        keep(&line, editing);
    }
}

/// A quad filling `rect` with `colour`, cut to `clip`.
const fn bar(rect: Rect2, clip: Rect2, colour: Colour) -> Quad {
    Quad { rect, clip, colour, border_width: 0.0, border_colour: colour, radius: 0.0, sample: Sample::Solid }
}
