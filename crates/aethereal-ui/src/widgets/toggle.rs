//! `Toggle`: a switch, on or off (ADR-0056: Stock widgets). Pressed like a button, or with Space while it has focus, it
//! raises its `change` with the other of the two; the screen's method keeps it, and the view shows it. It draws a track
//! in its style's colour, strong when on and faint when off, with a knob at the end it stands at.

use super::press::{Press, shade};
use crate::fonts::Measuring;
use crate::input::{Event, Hears, Input, Key, Pointing, Raised, Raising, Used};
use crate::style::Style;
use crate::widget::{Arrange, Draws, Laid, Offer, Quad, Sample, Widget};
use aethereal_math::{Rect2, Vec2};
use aethereal_render::draw_list::Pointer;
use aethereal_render::text::Colour;
use aethereal_window::Cursor;

/// The key that flips a toggle with focus.
const FLIPS: &[&str] = &["Space"];

/// How far a toggle's knob is inset from its track's edge, in logical pixels.
const INSET: f32 = 2.0;

/// How strong a toggle's track is while it is off, of its colour's alpha.
const OFF_STRENGTH: f32 = 0.35;

/// A switch that is `on` or not, raising `change` with the other when pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Toggle<E> {
    pub on: bool,
    pub change: E,
}

impl<E> Toggle<E> {
    /// A toggle that is `on` or not, raising `change`.
    #[must_use]
    pub const fn new(on: bool, change: E) -> Self {
        Self { on, change }
    }
}

impl<E: Event> Raising for Toggle<E> {
    type Event = E;
}

impl<E: Event> Widget for Toggle<E> {
    type State = Press;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    /// Two ems across and one down, at its style's font size.
    fn measure(&self, style: &Style, measuring: &mut Measuring<'_>, _: Offer) -> Vec2 {
        let em = style.font_size * measuring.scale();

        Vec2::new(em * 2.0, em)
    }

    fn draw(&self, press: &Press, style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        let track = laid.content;
        let strength = if self.on {
            1.0
        } else {
            OFF_STRENGTH
        };
        let colour = Colour { alpha: style.colour.alpha * strength, ..style.colour };
        let round = track.height() / 2.0;
        draws.quad(Quad {
            rect: track,
            clip: laid.clip,
            colour,
            border_width: 0.0,
            border_colour: colour,
            radius: round,
            sample: Sample::Solid,
        });
        let inset = INSET * laid.scale;
        let side = inset.mul_add(-2.0, track.height()).max(0.0);
        let x = if self.on {
            track.max.x - inset - side
        } else {
            track.min.x + inset
        };
        let knob = Rect2::from_origin_size(Vec2::new(x, track.min.y + inset), Vec2::new(side, side));
        draws.quad(Quad {
            rect: knob,
            clip: laid.clip,
            colour: Colour::WHITE,
            border_width: 0.0,
            border_colour: Colour::WHITE,
            radius: side / 2.0,
            sample: Sample::Solid,
        });
        shade(*press, style, laid, draws);
    }

    fn pointer(&self, _: &Press) -> Pointer {
        Pointer::Blocks(Cursor::Hand)
    }

    fn focusable(&self) -> bool {
        true
    }

    fn hears(&self) -> Hears {
        Hears::Keys(FLIPS)
    }

    fn pointed(&self, press: &mut Press, pointing: Pointing, input: &mut Input<'_, '_>) {
        let clicked = press.pointed(pointing);
        if clicked {
            input.raise(Raised::Flag(self.change, !self.on));
        }
    }

    fn key(&self, _: &mut Press, key: &Key, input: &mut Input<'_, '_>) -> Used {
        let flips = FLIPS.contains(&key.code.as_str());
        if !flips {
            return Used::Passed;
        }
        if !key.repeat {
            input.raise(Raised::Flag(self.change, !self.on));
        }

        Used::Used
    }
}
