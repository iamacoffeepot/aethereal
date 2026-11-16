//! [`Hint`]: the completion shown ahead of the console's line, faint, over its text input (ADR-0053: The band and the
//! console). It is laid out as [`Printed`] is, on one line, and draws only where that line fits its box: where the line
//! and its completion run past the text input's width, the text input wraps the line and the hint, unwrapped, would
//! stand apart from it, so it shows nothing. Whether it fits is the font's measure of its runs, as the library takes it.

use super::Printed;
use aethereal_math::Vec2;
use aethereal_render::text::{Run, Wrap};
use aethereal_ui::{Arrange, Draws, Laid, Measuring, Offer, Stateless, Style, Widget};

/// Text in pieces of their own colours, on one line, drawn only where that line fits its box.
#[derive(Debug, Clone, PartialEq)]
pub struct Hint {
    pub runs: Vec<Run>,
}

impl Hint {
    /// It as the printed text it draws as.
    fn printed(&self) -> Printed {
        Printed { runs: self.runs.clone() }
    }
}

impl Widget for Hint {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    fn measure(&self, style: &Style, measuring: &mut Measuring<'_>, offer: Offer) -> Vec2 {
        Widget::measure(&self.printed(), style, measuring, offer)
    }

    fn least(&self, style: &Style, measuring: &mut Measuring<'_>) -> Vec2 {
        Widget::least(&self.printed(), style, measuring)
    }

    /// As [`Printed`] draws it, where its one line is no wider than its box; nothing where it is, or before its font's
    /// metrics come.
    fn draw(&self, _: &Stateless, style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        let size = style.font_size * laid.scale;
        let Ok(measured) = draws.measuring().text(style.font, size, &self.runs, Wrap::None) else {
            return;
        };
        if measured.width > laid.content.width() {
            return;
        }

        Widget::draw(&self.printed(), &Stateless, style, laid, draws);
    }
}
