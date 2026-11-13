//! A meter's graph or bar as a primitive of the widget library (ADR-0056: Your own widgets): what it draws, as the view
//! builds it each `Prepare`, drawn into the box the library laid it out at, across the right column to its gutter and
//! inset from its row's top and foot.

use super::Held;
use super::bars::{TROUGH_OPACITY, bar};
use crate::binned::Spanned;
use crate::chart::{Fill, push};
use crate::colour::STONE;
use crate::frames::Slide;
use crate::layout::Scale;
use crate::lines;
use crate::widgets::paint;
use aethereal_math::Rect2;
use aethereal_ui::{Arrange, Draws, Laid, Stateless, Style, Widget};

/// What a meter's row draws: its last minute's columns and where they stand; its bytes against their limit; or nothing,
/// a meter with a limit before the engine's figures are read.
#[derive(Debug, Clone, PartialEq)]
pub enum MeterDrawn {
    History { columns: Vec<Spanned<u64>>, slide: Slide },
    Bar(Held),
    Nothing,
}

/// A meter's graph or bar: `drawn`, as wide and tall as the box it stretches to.
#[derive(Debug, Clone, PartialEq)]
pub struct MeterGraph {
    pub drawn: MeterDrawn,
}

impl Widget for MeterGraph {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    fn draw(&self, _: &Stateless, _: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        let mut fills = Vec::new();
        drawn(&mut fills, laid.content, Scale(laid.scale), &self.drawn);

        paint(draws, fills, laid.clip);
    }
}

/// Appends to `fills` what `drawn` says in `area` at `scale`: a trough of faint stone `area`'s size, and over it the
/// history's columns, drawn as the lines' rows are, a stone bar a column, or the bar's fill against its limit.
pub fn drawn(fills: &mut Vec<Fill>, area: Rect2, scale: Scale, drawn: &MeterDrawn) {
    match drawn {
        MeterDrawn::History { columns, slide } => {
            push(fills, area, STONE.at(TROUGH_OPACITY));
            lines::row(fills, area, scale, *slide, &columns.iter().copied());
        }
        MeterDrawn::Bar(held) => bar(fills, area, *held),
        MeterDrawn::Nothing => {}
    }
}
