//! The lines' rows as a primitive of the widget library (ADR-0056: Your own widgets): the three rows' columns and where
//! they stand, as the view builds them each `Prepare`, drawn into the box the library laid them out at, beneath the
//! GPU's graph and as wide, as tall as the three lines of the gutter beside them.

use super::{Values, rows};
use crate::binned::Spanned;
use crate::chart::Fill;
use crate::frames::Slide;
use crate::layout::Scale;
use crate::widgets::paint;
use aethereal_ui::{Arrange, Draws, Laid, Stateless, Style, Widget};

/// What the lines' rows show: their columns, newest first, and where they stand.
#[derive(Debug, Clone, PartialEq)]
pub struct LinesShown {
    pub columns: Vec<Spanned<Values>>,
    pub slide: Slide,
}

/// The lines' rows: `shown`, as tall as the box they stretch to.
#[derive(Debug, Clone, PartialEq)]
pub struct LineRows {
    pub shown: LinesShown,
}

impl Widget for LineRows {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    /// Nothing of it is laid out: it stretches to the box its style gives it, in which its rows slide every frame.
    fn same_layout(&self, _: &Self) -> bool {
        true
    }

    fn draw(&self, _: &Stateless, _: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        let LinesShown { columns, slide } = &self.shown;
        let mut fills: Vec<Fill> = Vec::new();
        rows(&mut fills, laid.content, Scale(laid.scale), *slide, &columns.iter().copied());

        paint(draws, fills, laid.clip);
    }
}
