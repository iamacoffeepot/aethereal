//! `TextInput`: text typed into, on one line or several (ADR-0056: Stock widgets; ADR-0051). Its text is the screen's:
//! each edit raises its `change` with the text as edited, which the screen's method keeps and the view shows, and Enter
//! raises its `submit` with the text. Its caret, its selection, the first line it shows, and what the input method is
//! composing are its own state, kept by its key.
//!
//! It wraps its text at its width and grows from one line to `most_lines`, past which it shows the lines around its
//! caret and slides through the rest as the caret moves, so no line is out of the caret's and the pointer's reach.
//!
//! With focus it hears every key, blocking, and the window's input method is allowed: typed text goes in at the caret,
//! in place of the selection, and what the input method composes shows there, underlined, until it commits. Enter
//! submits and Shift-Enter breaks the line, or the other way round, as its `enter` says. Its keys:
//!
//! - Left and Right move a character, and a word with Control or, on a Mac, Option; Up and Down move a line, keeping
//!   the place along the line they started from, and pass the key on from the first line up and the last line down,
//!   as a console steps its history then;
//! - Home and End, or Command with Left and Right, move to the start and end of the line as drawn; Control with Home
//!   and End, or Command with Up and Down, to the start and end of the text;
//! - Shift with any move takes the selection with it; Control or Command with A selects the whole;
//! - Backspace and Delete delete a character, and a word with Control or Option;
//! - Control or Command with C copies the selection, with X cuts it, and with V pastes what the clipboard holds at the
//!   caret as it stands once the clipboard answers (ADR-0056: Copy and paste).
//!
//! A key that types text is used, so a letter typed never reaches the screen's hotkeys; a key it does not use, Tab or
//! Escape or a function key, goes on to the focus keys and the screen. A press puts the caret under the pointer and a
//! drag from it selects, a double click selects a word and a triple click the line between breaks.
//!
//! A text its screen sets that is not its own last edit, a line recalled or completed, puts its caret at the text's
//! end. Undoing an edit is left out: the input keeps no history of its edits to step back through.

pub(super) mod edit;
mod shown;
#[cfg(test)]
mod tests;

use edit::{Edit, Selecting};
use shown::{Set, Shown};

use crate::fonts::{Measuring, Unmeasured};
use crate::input::{Event, Hears, Input, Key, Pointing, Raised, Raises, Raising, Used};
use crate::style::Style;
use crate::widget::{Arrange, Draws, Laid, Offer, Offered, Quad, Sample, TextDraw, Widget};
use aethereal_data::Plain;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::draw_list::Pointer;
use aethereal_render::text::{Align, Colour, Outline, Run};
use aethereal_window::{Button, Cursor};

/// How wide a text input is when nothing sizes it, in ems of its style's font.
const WIDTH_EMS: f32 = 10.0;

/// How wide its caret is, and how thick the line under what is composing, in logical pixels.
const CARET_WIDTH: f32 = 1.0;

/// How strong its selection is, of its style's colour's alpha.
const SELECTION_STRENGTH: f32 = 0.3;

/// Text typed into, raising `change` with each edit and, when it raises one, `submit` as `enter` says: markup's
/// `submit => …` sets it, and markup that leaves it out with `..` raises nothing. It grows to `most_lines` lines, at
/// least one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextInput<E> {
    pub text: String,
    pub change: E,
    pub submit: Raises<E>,
    pub most_lines: u32,
    pub enter: Enter,
}

/// What Enter does in a text input, and Shift-Enter the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Enter {
    /// Enter submits, and Shift-Enter breaks the line, as a chat's line does.
    #[default]
    Submits,
    /// Enter breaks the line, and Shift-Enter submits, as an editor does.
    Breaks,
}

/// A text input's caret and the other end of its selection, byte offsets into its text; whether a press on it is
/// selecting as the pointer drags; its text as it last edited it, which tells an edit of its own from a text its
/// screen set; the first line it shows; where along a line a move up or down keeps to; and what the input method is
/// composing.
#[derive(Debug, Clone, PartialEq, Default, Plain)]
#[kind(name = "aether.ui.text_input")]
pub struct Editing {
    pub caret: u32,
    pub anchor: u32,
    pub dragging: bool,
    pub edited: String,
    pub top: u32,
    pub along: Along,
    pub composing: Composing,
}

/// Where along a line a move up or down puts the caret.
#[derive(Debug, Clone, Copy, PartialEq, Default, Plain)]
pub enum Along {
    /// Where the caret stands: any move but up or down, and any edit, leaves it there.
    #[default]
    Caret,
    /// This many logical pixels along, where a run of moves up and down began, so a short line passed through does not
    /// pull the caret to its end for good.
    Kept { x: f32 },
}

/// What the window's input method is composing at the caret.
#[derive(Debug, Clone, PartialEq, Eq, Default, Plain)]
pub enum Composing {
    /// Nothing.
    #[default]
    Closed,
    /// `text`, not yet committed, its own caret at the byte `caret` of it.
    Open { text: String, caret: u32 },
}

/// Which end of a line a move goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edge {
    Start,
    End,
}

/// Which way a move up or down goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Vertical {
    Up,
    Down,
}

impl<E> TextInput<E> {
    /// A text input of one line showing `text`, raising `change`, and no submit.
    #[must_use]
    pub fn new(text: &str, change: E) -> Self {
        Self { text: String::from(text), change, submit: Raises::Nothing, most_lines: 1, enter: Enter::Submits }
    }

    /// It, raising `submit` as its `enter` says.
    #[must_use]
    pub fn submit(self, submit: E) -> Self {
        Self { submit: Raises::Event(submit), ..self }
    }

    /// It, growing to `most_lines` lines.
    #[must_use]
    pub fn most_lines(self, most_lines: u32) -> Self {
        Self { most_lines, ..self }
    }

    /// It, Enter doing as `enter` says.
    #[must_use]
    pub fn enter(self, enter: Enter) -> Self {
        Self { enter, ..self }
    }

    /// Its text as `editing` stands in it: a text its screen set, not one it last edited, with the caret at its end, as
    /// a line recalled from a history or completed is typed on from its end.
    fn edit(&self, editing: &Editing) -> Edit {
        if editing.edited != self.text {
            return Edit::new(&self.text, self.text.len(), self.text.len());
        }

        Edit::new(&self.text, offset(editing.caret), offset(editing.anchor))
    }

    /// The most lines it shows, at least one.
    fn rows(&self) -> u32 {
        self.most_lines.max(1)
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

/// What a text input reached by `input` shows of `edit`'s text, the first line kept at `top`.
fn shown(edit: &Edit, top: u32, input: &mut Input<'_, '_>) -> Result<Shown, Unmeasured> {
    let set = Set::of(input.style(), input.laid().scale);
    let content = input.laid().content;

    Shown::of(set, input.measuring(), &edit.text, content, (top, edit.caret))
}

/// Keeps `edit`'s caret, anchor, and text in `editing`, with the first line shown moved to show the caret, as `input`
/// lays it out.
fn keep(edit: &Edit, editing: &mut Editing, input: &mut Input<'_, '_>) {
    editing.caret = kept(edit.caret);
    editing.anchor = kept(edit.anchor);
    editing.edited.clone_from(&edit.text);
    if let Ok(shown) = shown(edit, editing.top, input) {
        editing.top = shown.kept_top();
    }
}

/// A text with what is composing at its caret: the text drawn, where the caret is drawn in it, and where the
/// composition runs, empty when nothing is composing.
fn composed(edit: &Edit, composing: &Composing) -> (String, usize, (usize, usize)) {
    let Composing::Open { text, caret } = composing else {
        return (edit.text.clone(), edit.caret, (edit.caret, edit.caret));
    };
    let mut drawn = edit.text.clone();
    drawn.insert_str(edit.caret, text);
    let within = edit::boundary(text, offset(*caret));

    (drawn, edit.caret + within, (edit.caret, edit.caret + text.len()))
}

impl<E: Event> TextInput<E> {
    /// Raises `change` with `edit`'s text, when an edit changed it.
    fn changed(&self, edit: &Edit, input: &mut Input<'_, '_>) {
        if edit.text != self.text {
            input.raise(Raised::Text(self.change, edit.text.clone()));
        }
    }

    /// Takes in Enter, with Shift held or not: it submits or breaks the line, as its `enter` says.
    fn entered(&self, shifted: bool, edit: &mut Edit, input: &mut Input<'_, '_>) {
        let submits = match self.enter {
            Enter::Submits => !shifted,
            Enter::Breaks => shifted,
        };
        if !submits {
            edit.typed("\n");
            return;
        }
        if let Raises::Event(submit) = self.submit {
            input.raise(Raised::Text(submit, self.text.clone()));
        }
    }
}

/// Moves `edit`'s caret a line up or down from where it stands, to the place along it nearest `column`'s, as `input`
/// lays it out: whether it moved, as there is no line past the first or the last.
fn vertical(
    edit: &mut Edit,
    editing: &mut Editing,
    (toward, selecting): (Vertical, Selecting),
    input: &mut Input<'_, '_>,
) -> Used {
    let scale = input.laid().scale;
    let Ok(shown) = shown(edit, editing.top, input) else {
        return Used::Passed;
    };
    let (line, x) = shown.carets.place_of(edit.caret);
    let target = match toward {
        Vertical::Up => line.checked_sub(1),
        Vertical::Down => Some(line + 1),
    };
    let Some(to) = target.and_then(|target| shown.carets.lines.get(target)) else {
        return Used::Passed;
    };
    let along = match editing.along {
        Along::Kept { x } => x * scale,
        Along::Caret => x,
    };
    edit.move_to(to.nearest(along), selecting);
    editing.along = Along::Kept { x: along / scale };

    Used::Used
}

/// Where a move to the `edge` of the line the caret is on, as drawn, lands, as `input` lays it out: where
/// the caret stands, before the font's metrics come.
fn line_end(edit: &Edit, editing: &Editing, edge: Edge, input: &mut Input<'_, '_>) -> usize {
    let Ok(shown) = shown(edit, editing.top, input) else {
        return edit.caret;
    };
    let line = shown.line_at(edit.caret);

    match edge {
        Edge::Start => line.start,
        Edge::End => line.last(),
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

    /// The width it is offered, or ten ems with none, and as many lines down as its text wraps to at that width, from
    /// one up to its most.
    fn measure(&self, style: &Style, measuring: &mut Measuring<'_>, offer: Offer) -> Vec2 {
        let set = Set::of(style, measuring.scale());
        let width = match offer.width {
            Offered::Exactly(width) | Offered::AtMost(width) => width,
            Offered::MinContent | Offered::MaxContent => set.size * WIDTH_EMS,
        };
        let runs = [Run::new(&self.text, set.colour)];
        let Ok(measured) = measuring.text(set.font, set.size, &runs, Set::wrap(width)) else {
            return Vec2::new(width, 0.0);
        };
        let lines = measured.lines.max(1);
        // A text input's lines count far fewer than an f32 holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let height = measured.height / lines as f32 * lines.min(self.rows()) as f32;

        Vec2::new(width, height)
    }

    fn draw(&self, editing: &Editing, style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        let focused = draws.focused();
        let set = Set::of(style, laid.scale);
        let edit = self.edit(editing);
        let (written, caret, composing) = composed(&edit, &editing.composing);
        let content = laid.content;
        let clip = laid.clip.intersection(content);
        let Ok(shown) = Shown::of(set, draws.measuring(), &written, content, (editing.top, caret)) else {
            return;
        };
        let height = shown.carets.line_height;
        let thickness = CARET_WIDTH * laid.scale;
        let left = content.min.x;

        // The selection shows only while it has focus, and gives way to what is composing in its place.
        let (start, end) = edit.selected();
        let quiet = composing.0 == composing.1;
        let selecting = focused && quiet && start != end;
        let highlight = Colour { alpha: style.colour.alpha * SELECTION_STRENGTH, ..style.colour };
        for (number, line) in shown.visible() {
            let top = shown.y_of(number);
            let span = |(from, to): (usize, usize)| {
                let (from, to) = (from.max(line.start), to.min(line.end));
                let rect = Rect2 {
                    min: Vec2::new(left + line.x_of(from), top),
                    max: Vec2::new(left + line.x_of(to), top + height),
                };
                (from < to).then_some(rect)
            };
            if selecting && let Some(rect) = span((start, end)) {
                draws.quad(bar(rect, clip, highlight));
            }
            if let Some(rect) = span(composing) {
                let under = Rect2 { min: Vec2::new(rect.min.x, rect.max.y - thickness), max: rect.max };
                draws.quad(bar(under, clip, set.colour));
            }
        }
        draws.text(TextDraw {
            at: Vec2::new(left, shown.y_of(0)),
            clip,
            font: set.font,
            size: set.size,
            wrap: Set::wrap(content.width()),
            align: Align::Left,
            outline: Outline::None,
            runs: vec![Run::new(&written, set.colour)],
        });
        if focused {
            let (line, x) = shown.carets.place_of(caret);
            let rect = Rect2::from_origin_size(Vec2::new(left + x, shown.y_of(line)), Vec2::new(thickness, height));
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
        let mut edit = self.edit(editing);
        let Ok(shown) = shown(&edit, editing.top, input) else {
            return;
        };
        match pointing {
            Pointing::Down { at, button: Button::Main, presses } => {
                let pressed = shown.pressed(at);
                match presses {
                    0 | 1 => edit.move_to(pressed, Selecting::Collapse),
                    2 => {
                        let (start, end) = edit.word_around(pressed);
                        edit.select(start, end);
                    }
                    _ => {
                        let (start, end) = edit.line_around(pressed);
                        edit.select(start, end);
                    }
                }
                editing.dragging = true;
            }
            Pointing::Moved { at } if editing.dragging => edit.move_to(shown.pressed(at), Selecting::Extend),
            Pointing::Up { button: Button::Main, .. } => editing.dragging = false,
            Pointing::Entered
            | Pointing::Left
            | Pointing::Moved { .. }
            | Pointing::Down { .. }
            | Pointing::Up { .. } => return,
        }
        editing.along = Along::Caret;
        keep(&edit, editing, input);
    }

    fn key(&self, editing: &mut Editing, key: &Key, input: &mut Input<'_, '_>) -> Used {
        let mut edit = self.edit(editing);
        let held = key.held;
        let selecting = if held.shift {
            Selecting::Extend
        } else {
            Selecting::Collapse
        };
        // A word is Control's on most keyboards, and Option's on a Mac, whose Command moves to a line's ends.
        let by_word = held.control || held.alt;
        let (start, end) = edit.selected();
        let collapses = selecting == Selecting::Collapse && edit.selects();
        let caret = edit.caret;
        let along = editing.along;
        editing.along = Along::Caret;
        match key.code.as_str() {
            "KeyA" if held.command() => edit.all(),
            "KeyC" if held.command() => {
                if edit.selects() {
                    input.copy(String::from(edit.selection()));
                }
            }
            "KeyX" if held.command() => {
                if edit.selects() {
                    input.cut(String::from(edit.selection()));
                }
            }
            "KeyV" if held.command() => input.paste(),
            "Backspace" => {
                let to = if by_word {
                    edit.word_back(caret)
                } else {
                    edit.back(caret)
                };
                edit.delete(to);
            }
            "Delete" => {
                let to = if by_word {
                    edit.word_forward(caret)
                } else {
                    edit.forward(caret)
                };
                edit.delete(to);
            }
            "ArrowLeft" | "Home" if held.meta => {
                let to = line_end(&edit, editing, Edge::Start, input);
                edit.move_to(to, selecting);
            }
            "ArrowRight" | "End" if held.meta => {
                let to = line_end(&edit, editing, Edge::End, input);
                edit.move_to(to, selecting);
            }
            "ArrowLeft" if collapses => edit.move_to(start, selecting),
            "ArrowRight" if collapses => edit.move_to(end, selecting),
            "ArrowLeft" => {
                let to = if by_word {
                    edit.word_back(caret)
                } else {
                    edit.back(caret)
                };
                edit.move_to(to, selecting);
            }
            "ArrowRight" => {
                let to = if by_word {
                    edit.word_forward(caret)
                } else {
                    edit.forward(caret)
                };
                edit.move_to(to, selecting);
            }
            "ArrowUp" if held.meta => edit.move_to(0, selecting),
            "ArrowDown" if held.meta => edit.move_to(edit.text.len(), selecting),
            "Home" if held.control => edit.move_to(0, selecting),
            "End" if held.control => edit.move_to(edit.text.len(), selecting),
            "Home" => {
                let to = line_end(&edit, editing, Edge::Start, input);
                edit.move_to(to, selecting);
            }
            "End" => {
                let to = line_end(&edit, editing, Edge::End, input);
                edit.move_to(to, selecting);
            }
            "ArrowUp" | "ArrowDown" => {
                let toward = if key.code == "ArrowUp" {
                    Vertical::Up
                } else {
                    Vertical::Down
                };
                editing.along = along;
                let used = vertical(&mut edit, editing, (toward, selecting), input);
                if used == Used::Passed {
                    editing.along = Along::Caret;
                    return Used::Passed;
                }
            }
            "Enter" | "NumpadEnter" => self.entered(held.shift, &mut edit, input),
            _ if key.types() => {}
            _ => return Used::Passed,
        }
        self.changed(&edit, input);
        keep(&edit, editing, input);

        Used::Used
    }

    fn typed(&self, editing: &mut Editing, text: &str, input: &mut Input<'_, '_>) {
        let mut edit = self.edit(editing);
        edit.typed(text);
        editing.along = Along::Caret;
        self.changed(&edit, input);
        keep(&edit, editing, input);
    }

    fn composing(&self, editing: &mut Editing, text: &str, caret: u32, _: &mut Input<'_, '_>) {
        editing.composing = if text.is_empty() {
            Composing::Closed
        } else {
            Composing::Open { text: String::from(text), caret }
        };
    }
}

/// A quad filling `rect` with `colour`, cut to `clip`.
const fn bar(rect: Rect2, clip: Rect2, colour: Colour) -> Quad {
    Quad { rect, clip, colour, border_width: 0.0, border_colour: colour, radius: 0.0, sample: Sample::Solid }
}
