//! `Selectable`: a text in runs of colours of their own that its reader can select and copy (ADR-0056: Copy and paste),
//! as a transcript's messages and a console's rows are. It measures and draws as a [`Rich`] of its runs that wraps
//! does, in the same box, and over that a press puts a caret, a drag selects, a double click selects a word and a
//! triple click the line between breaks, and Control or Command with C copies the selection, with A selects the whole.
//! Its selection is drawn beneath its text in its style's colour, faint, as its runs carry their own; it shows while it
//! has its screen's focus, which a press gives it so the keys reach it; Tab passes it, as it is read, not filled in.
//! What it selects and copies is its runs' text, joined: their colours are only how it is drawn.
//!
//! Selection runs within one `Selectable`: a press in another starts that one's.

#[cfg(test)]
mod tests;

use super::rich::{Reading, Rich};
use super::text::{Width, fitted, rim};
use super::text_input::edit::{Edit, Selecting as Moving};
use crate::fonts::{Measuring, Unmeasured};
use crate::input::{Hears, Input, Key, Pointing, Used};
use crate::style::Style;
use crate::widget::{Arrange, Draws, Laid, Offer, Quad, Sample, Widget};
use aethereal_data::Plain;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::draw_list::Pointer;
use aethereal_render::text::{Align, CaretLine, Carets, Colour, Run, Wrap};
use aethereal_window::{Button, Cursor};

/// How strong its selection is, of its style's colour's alpha: what a screen choosing that colour reckons with, so its
/// selection is seen on what it lies on.
pub const SELECTION_STRENGTH: f32 = 0.3;

/// Runs of text in colours of their own, drawn as a [`Rich`] of them that wraps is, that its reader can select and
/// copy.
#[derive(Debug, Clone, PartialEq)]
pub struct Selectable {
    pub runs: Vec<Run>,
}

/// A selectable text's caret and the other end of its selection, byte offsets into its runs' text joined, and whether a
/// press on it is selecting as the pointer drags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Plain)]
#[kind(name = "aether.ui.selecting")]
pub struct Selecting {
    pub caret: u32,
    pub anchor: u32,
    pub dragging: bool,
}

impl Selecting {
    /// Whether its reader has a selection standing in its text or is making one with a drag: what a screen that gives
    /// its focus elsewhere of its own accord leaves be, so the selection shows and the copy reaches it.
    #[must_use]
    pub const fn selects(self) -> bool {
        self.dragging || self.caret != self.anchor
    }
}

/// Its text as drawn in its content box: its lines, where the first starts, and the width its lines are set within.
struct Shown {
    carets: Carets,
    origin: Vec2,
    within: f32,
    align: Align,
}

impl Shown {
    /// How far right of its origin the line `line` starts, as it is aligned.
    fn shift(&self, line: &CaretLine) -> f32 {
        match self.align {
            Align::Left => 0.0,
            Align::Centre => (self.within - line.width) / 2.0,
            Align::Right => self.within - line.width,
        }
    }

    /// The boundary nearest the pointer at `at`: on the line under it, or the first or last line past them.
    fn pressed(&self, at: Vec2) -> usize {
        let height = self.carets.line_height.max(f32::MIN_POSITIVE);
        let last = self.carets.lines.len().saturating_sub(1);
        // A text's lines count far fewer than an f32 holds exactly, and a pointer far past them is clamped.
        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::cast_sign_loss)]
        let number = ((at.y - self.origin.y) / height).floor().clamp(0.0, last as f32) as usize;
        let line = &self.carets.lines[number];

        line.nearest(at.x - self.origin.x - self.shift(line))
    }
}

impl Selectable {
    /// It as the rich text it draws as: its runs, wrapping, with no links.
    fn rich(&self) -> Rich<()> {
        Rich { runs: self.runs.clone(), ..Rich::default() }
    }

    /// Its runs' text, joined: what its selection's offsets count and its copy writes.
    fn text(&self) -> String {
        self.runs.iter().map(|run| run.text.as_str()).collect()
    }

    /// Its text as `selecting` stands in it.
    fn edit(&self, selecting: Selecting) -> Edit {
        let at = |kept: u32| usize::try_from(kept).unwrap_or(usize::MAX);

        Edit::new(&self.text(), at(selecting.caret), at(selecting.anchor))
    }

    /// Its text as drawn in `content` styled `style` at `scale`: laid out as `Rich` lays it out, wrapped where it
    /// wraps.
    fn shown(
        &self,
        style: &Style,
        (content, scale): (Rect2, f32),
        measuring: &mut Measuring<'_>,
    ) -> Result<Shown, Unmeasured> {
        let pad = rim(style, scale);
        let width = pad.mul_add(-2.0, content.width());
        let (measured, wrap) = fitted(&self.runs, style, measuring, Width::Within(width))?;
        let within = match wrap {
            Wrap::None => measured.width,
            Wrap::At { width } => width,
        };
        let shift = match style.text_align {
            Align::Left => 0.0,
            Align::Centre => (width - within) / 2.0,
            Align::Right => width - within,
        };
        let carets = measuring.carets(style.font, style.font_size * scale, &self.text(), wrap)?;

        Ok(Shown { carets, origin: content.min + Vec2::new(pad + shift, pad), within, align: style.text_align })
    }
}

/// Keeps `edit`'s caret and anchor in `selecting`.
fn keep(edit: &Edit, selecting: &mut Selecting) {
    selecting.caret = u32::try_from(edit.caret).unwrap_or(u32::MAX);
    selecting.anchor = u32::try_from(edit.anchor).unwrap_or(u32::MAX);
}

impl Widget for Selectable {
    type State = Selecting;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    fn measure(&self, style: &Style, measuring: &mut Measuring<'_>, offer: Offer) -> Vec2 {
        Widget::measure(&self.rich(), style, measuring, offer)
    }

    fn least(&self, style: &Style, measuring: &mut Measuring<'_>) -> Vec2 {
        Widget::least(&self.rich(), style, measuring)
    }

    /// Its selection beneath its text in its style's colour, faint, while it has focus, then its runs as `Rich` draws
    /// them.
    fn draw(&self, selecting: &Selecting, style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        let edit = self.edit(*selecting);
        let lit = draws.focused() && edit.selects();
        let shown = self.shown(style, (laid.content, laid.scale), draws.measuring());
        if lit && let Ok(shown) = shown {
            let (start, end) = edit.selected();
            let colour = Colour { alpha: style.colour.alpha * SELECTION_STRENGTH, ..style.colour };
            let clip = laid.clip.intersection(laid.content);
            let height = shown.carets.line_height;
            for (number, line) in shown.carets.lines.iter().enumerate() {
                let (from, to) = (start.max(line.start), end.min(line.end));
                if from >= to {
                    continue;
                }
                let left = shown.origin.x + shown.shift(line);
                // A text's lines count far fewer than an f32 holds exactly.
                #[allow(clippy::cast_precision_loss)]
                let top = (number as f32).mul_add(height, shown.origin.y);
                let rect = Rect2 {
                    min: Vec2::new(left + line.x_of(from), top),
                    max: Vec2::new(left + line.x_of(to), top + height),
                };
                draws.quad(Quad {
                    rect,
                    clip,
                    colour,
                    border_width: 0.0,
                    border_colour: colour,
                    radius: 0.0,
                    sample: Sample::Solid,
                });
            }
        }
        Widget::draw(&self.rich(), &Reading::default(), style, laid, draws);
    }

    fn pointer(&self, _: &Selecting) -> Pointer {
        Pointer::Blocks(Cursor::Text)
    }

    fn focusable(&self) -> bool {
        true
    }

    fn tabbable(&self) -> bool {
        false
    }

    fn hears(&self) -> Hears {
        Hears::Keys(&["KeyA", "KeyC"])
    }

    fn pointed(&self, selecting: &mut Selecting, pointing: Pointing, input: &mut Input<'_, '_>) {
        let mut edit = self.edit(*selecting);
        let content = (input.laid().content, input.laid().scale);
        let style = input.style().clone();
        let Ok(shown) = self.shown(&style, content, input.measuring()) else {
            return;
        };
        match pointing {
            Pointing::Down { at, button: Button::Main, presses } => {
                let pressed = shown.pressed(at);
                let (start, end) = match presses {
                    0 | 1 => (pressed, pressed),
                    2 => edit.word_around(pressed),
                    _ => edit.line_around(pressed),
                };
                edit.select(start, end);
                selecting.dragging = true;
            }
            Pointing::Moved { at } if selecting.dragging => edit.move_to(shown.pressed(at), Moving::Extend),
            Pointing::Up { button: Button::Main, .. } => selecting.dragging = false,
            Pointing::Entered
            | Pointing::Left
            | Pointing::Moved { .. }
            | Pointing::Down { .. }
            | Pointing::Up { .. } => return,
        }
        keep(&edit, selecting);
    }

    fn key(&self, selecting: &mut Selecting, key: &Key, input: &mut Input<'_, '_>) -> Used {
        let mut edit = self.edit(*selecting);
        let command = key.held.command();
        match key.code.as_str() {
            "KeyA" if command => edit.all(),
            "KeyC" if command => {
                if edit.selects() {
                    input.copy(String::from(edit.selection()));
                }
            }
            _ => return Used::Passed,
        }
        keep(&edit, selecting);

        Used::Used
    }
}
