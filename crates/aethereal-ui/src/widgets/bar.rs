//! `Bar`: a filled fraction of its box, as a health or a cooldown is shown (ADR-0056: Stock widgets). Its style's
//! background is the track beneath, and its style's colour fills the fraction from the edge its fill starts at.

use crate::style::Style;
use crate::widget::{Arrange, Draws, Laid, Quad, Sample, Stateless, Widget};
use aethereal_math::{Rect2, Vec2};

/// A fraction from 0 to 1 of its box filled, growing as its fill says. A fraction outside them fills none or all.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bar {
    pub fraction: f32,
    pub fill: Fill,
}

/// Which way a bar fills as its fraction grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fill {
    /// From its left edge, rightward.
    Right,
    /// From its right edge, leftward.
    Left,
    /// From its top edge, downward.
    Down,
    /// From its bottom edge, upward.
    Up,
}

impl Widget for Bar {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    fn draw(&self, _: &Stateless, style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        let whole = laid.content;
        let fraction = if self.fraction.is_nan() {
            0.0
        } else {
            self.fraction.clamp(0.0, 1.0)
        };
        let across = whole.width() * fraction;
        let down = whole.height() * fraction;
        let filled = match self.fill {
            Fill::Right => Rect2 { min: whole.min, max: Vec2::new(whole.min.x + across, whole.max.y) },
            Fill::Left => Rect2 { min: Vec2::new(whole.max.x - across, whole.min.y), max: whole.max },
            Fill::Down => Rect2 { min: whole.min, max: Vec2::new(whole.max.x, whole.min.y + down) },
            Fill::Up => Rect2 { min: Vec2::new(whole.min.x, whole.max.y - down), max: whole.max },
        };
        draws.quad(Quad {
            rect: filled,
            clip: laid.clip,
            colour: style.colour,
            border_width: 0.0,
            border_colour: style.colour,
            radius: style.radius * laid.scale,
            sample: Sample::Solid,
        });
    }
}
