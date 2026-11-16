//! `Selectable`: a text in runs of colours of their own that its reader can select and copy (ADR-0056: Copy and paste),
//! as a transcript's messages and a console's rows are. It measures and draws as a [`Rich`] of its runs that wraps
//! does, in the same box.
//!
//! The selection is its screen's, not its own: a press on it puts the screen's caret in its text, a drag selects on
//! through it and through the texts after or before it in the scroll it stands in, a double click selects a word and a
//! triple click the line between breaks, and Control or Command with C copies the screen's selection, with A selects
//! all the scroll holds, or the whole of it outside a scroll. The part of it the selection holds is drawn beneath its
//! text in the colour of the scroll it runs within, faint, as its runs carry their own; a press on it takes the focus,
//! so its keys reach it, and Tab passes it, as it is read, not filled in. What it selects and copies is its runs' text,
//! joined: their colours are only how it is drawn.

#[cfg(test)]
mod tests;

use super::rich::{Flow, Reading, Rich};
use super::text::read::{self, joined};
use crate::fonts::Measuring;
use crate::input::{Hears, Input, Key, Used};
use crate::style::Style;
use crate::widget::{Arrange, Draws, Laid, Offer, Reads, Stateless, Widget};
use aethereal_math::Vec2;
use aethereal_render::draw_list::Pointer;
use aethereal_render::text::Run;
use aethereal_window::Cursor;

/// How strong a selection is, of the colour it is drawn in's alpha: what a screen choosing that colour reckons with, so
/// its selection is seen on what it lies on.
pub const SELECTION_STRENGTH: f32 = 0.3;

/// Runs of text in colours of their own, drawn as a [`Rich`] of them that wraps is, that its reader can select and
/// copy.
#[derive(Debug, Clone, PartialEq)]
pub struct Selectable {
    pub runs: Vec<Run>,
}

impl Selectable {
    /// It as the rich text it draws as: its runs, wrapping, with no links.
    fn rich(&self) -> Rich<()> {
        Rich { runs: self.runs.clone(), ..Rich::default() }
    }
}

impl Widget for Selectable {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    fn measure(&self, style: &Style, measuring: &mut Measuring<'_>, offer: Offer) -> Vec2 {
        Widget::measure(&self.rich(), style, measuring, offer)
    }

    fn least(&self, style: &Style, measuring: &mut Measuring<'_>) -> Vec2 {
        Widget::least(&self.rich(), style, measuring)
    }

    /// The part of it its screen's selection holds, beneath its text, then its runs, as `Rich` draws them.
    fn draw(&self, _: &Stateless, style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        Widget::draw(&self.rich(), &Reading::default(), style, laid, draws);
    }

    fn pointer(&self, _: &Stateless) -> Pointer {
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

    /// Control or Command with A selects all the scroll it stands in holds, and with C copies its screen's selection.
    fn key(&self, _: &mut Stateless, key: &Key, input: &mut Input<'_, '_>) -> Used {
        input.selection_key(key)
    }

    fn reads(&self) -> Reads {
        Reads::Selectable
    }

    fn read(&self) -> String {
        joined(&self.runs)
    }

    fn boundary(&self, at: Vec2, style: &Style, laid: &Laid, measuring: &mut Measuring<'_>) -> u32 {
        read::boundary(&self.runs, Flow::Wraps, at, (style, laid), measuring)
    }
}
