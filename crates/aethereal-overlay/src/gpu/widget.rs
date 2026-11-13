//! The GPU's graph as a primitive of the widget library (ADR-0056: Your own widgets): its columns, where they stand,
//! and the budget, as the view builds them each `Prepare`, drawn into the box the library laid it out at, beneath the
//! frame chart and as wide, so each column stands under the frame chart's of the same span.

use super::{Stack, graph};
use crate::binned::Spanned;
use crate::chart::Fill;
use crate::frames::Slide;
use crate::layout::Scale;
use crate::widgets::paint;
use aethereal_math::Vec2;
use aethereal_render::stats::FrameBudget;
use aethereal_ui::{Arrange, Draws, Laid, Measuring, Offer, Stateless, Style, Widget};

/// What the GPU's graph shows: its columns, newest first, where they stand, and the budget.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuShown {
    pub columns: Vec<Spanned<Stack>>,
    pub slide: Slide,
    pub budget: FrameBudget,
}

/// The GPU's graph: `shown`, `tall` pixels tall.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuGraph {
    pub shown: GpuShown,
    pub tall: f32,
}

impl Widget for GpuGraph {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    fn measure(&self, _: &Style, _: &mut Measuring<'_>, _: Offer) -> Vec2 {
        Vec2::new(0.0, self.tall)
    }

    fn draw(&self, _: &Stateless, _: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        let GpuShown { columns, slide, budget } = &self.shown;
        let mut fills: Vec<Fill> = Vec::new();
        graph(&mut fills, laid.content, Scale(laid.scale), *slide, *budget, columns.iter().copied());

        paint(draws, fills, laid.clip);
    }
}
