//! The frame chart as a primitive of the widget library (ADR-0056: Your own widgets): what it shows, the columns kept,
//! where they stand, the budget, and the marks, as the view builds it each `Prepare`, drawn into the box the library
//! laid it out at, as tall as the window's share it is given and as wide as its row leaves it.

use super::{Fill, marks, records};
use crate::frames::{Column, Slide};
use crate::layout::Scale;
use crate::widgets::paint;
use aethereal_math::Vec2;
use aethereal_render::stats::FrameBudget;
use aethereal_ui::{Arrange, Draws, Laid, Measuring, Offer, Stateless, Style, Widget};

/// What the frame chart shows: its columns, newest first, where they stand, the budget they are drawn against, and the
/// program's time of each mark kept.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartShown {
    pub columns: Vec<Column>,
    pub slide: Slide,
    pub budget: FrameBudget,
    pub marks: Vec<u64>,
}

/// The frame chart: `shown`, `tall` pixels tall.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameChart {
    pub shown: ChartShown,
    pub tall: f32,
}

impl Widget for FrameChart {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    fn measure(&self, _: &Style, _: &mut Measuring<'_>, _: Offer) -> Vec2 {
        Vec2::new(0.0, self.tall)
    }

    fn draw(&self, _: &Stateless, _: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        let ChartShown { columns, slide, budget, marks: times } = &self.shown;
        let scale = Scale(laid.scale);
        let mut fills: Vec<Fill> = Vec::new();
        records(&mut fills, laid.content, scale, columns.iter().copied(), *slide, *budget);
        marks(&mut fills, laid.content, scale, *slide, times.iter().copied());

        paint(draws, fills, laid.clip);
    }
}
