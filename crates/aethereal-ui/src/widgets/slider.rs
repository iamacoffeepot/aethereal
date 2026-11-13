//! `Slider`: a value in a range, set by dragging along it (ADR-0056: Stock widgets). A press on it sets the value under
//! the pointer and holds it: every move sets it again, wherever the pointer goes, as the window keeps sending moves to
//! what a press reached (ADR-0055: capture), and each value that differs from the one shown raises its `change`. With
//! focus, the arrows step it, Page Up and Page Down step it ten times, and Home and End set its ends.
//!
//! It draws a track in its style's colour, faint, filled strongly to its value, with a knob at the value that grows
//! while the pointer is over it or drags it.

use super::press::ring;
use crate::fonts::Measuring;
use crate::input::{Event, Hears, Input, Key, Pointing, Raised, Raising, Used};
use crate::style::Style;
use crate::widget::{Arrange, Draws, Laid, Offer, Offered, Quad, Sample, Widget};
use aethereal_data::Plain;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::draw_list::Pointer;
use aethereal_render::text::Colour;
use aethereal_window::{Button, Cursor};

/// The keys that step a slider with focus.
const STEPS: &[&str] = &["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End"];

/// How many steps Page Up and Page Down take.
const PAGE: f32 = 10.0;

/// How many steps a slider with no step of its own is divided into, for its keys.
const STEPS_WITHOUT_STEP: f32 = 10.0;

/// How wide a slider is when nothing sizes it, in ems of its style's font.
const WIDTH_EMS: f32 = 10.0;

/// How strong a slider's track is, of its colour's alpha, beyond its value.
const TRACK_STRENGTH: f32 = 0.35;

/// How much a slider's knob grows while the pointer is over it or drags it, in logical pixels.
const GROWTH: f32 = 2.0;

/// A value between `min` and `max`, set along the slider's box from left to right, raising `change` with each new
/// value; each a whole number of `step`s from `min`, or any value with a step of zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slider<E> {
    pub value: f32,
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub change: E,
}

/// Whether the pointer is over a slider, and whether a press on it drags it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Plain)]
#[kind(name = "aether.ui.slider")]
pub struct Sliding {
    pub hovered: bool,
    pub dragging: bool,
}

impl<E> Slider<E> {
    /// A slider at `value` between `min` and `max` in steps of `step`, raising `change`.
    #[must_use]
    pub const fn new(value: f32, min: f32, max: f32, step: f32, change: E) -> Self {
        Self { value, min, max, step, change }
    }

    /// Its least and its greatest value, whichever way round they were given.
    fn ends(&self) -> (f32, f32) {
        (self.min.min(self.max), self.max.max(self.min))
    }

    /// `value` within its range, a whole number of steps from its least.
    fn fitted(&self, value: f32) -> f32 {
        let (least, most) = self.ends();
        let stepped = self.step > 0.0;
        let snapped = if stepped {
            ((value - least) / self.step).round().mul_add(self.step, least)
        } else {
            value
        };

        snapped.max(least).min(most)
    }

    /// How far its value is from its least to its greatest, from 0 to 1.
    fn fraction(&self) -> f32 {
        let (least, most) = self.ends();
        let span = most - least;
        let flat = span <= 0.0;
        if flat {
            return 0.0;
        }

        ((self.value - least) / span).clamp(0.0, 1.0)
    }

    /// How far one of its keys steps it: its step, or a tenth of its range with none.
    fn stride(&self) -> f32 {
        let (least, most) = self.ends();
        let stepped = self.step > 0.0;
        if stepped {
            self.step
        } else {
            (most - least) / STEPS_WITHOUT_STEP
        }
    }
}

impl<E: Event> Slider<E> {
    /// Raises `change` with `value`, fitted to the range, when it is not the value shown.
    fn set(&self, value: f32, input: &mut Input<'_, '_>) {
        let value = self.fitted(value);
        let moved = value.to_bits() != self.value.to_bits();
        if moved {
            input.raise(Raised::Number(self.change, value));
        }
    }

    /// Sets the value under `at`, across its content box, the pointer's place in its own coordinates.
    fn dragged(&self, at: Vec2, input: &mut Input<'_, '_>) {
        let content = input.laid().content;
        let across = content.width();
        let narrow = across <= 0.0;
        let fraction = if narrow {
            0.0
        } else {
            ((at.x - content.min.x) / across).clamp(0.0, 1.0)
        };
        let (least, most) = self.ends();
        self.set(fraction.mul_add(most - least, least), input);
    }
}

impl<E: Event> Raising for Slider<E> {
    type Event = E;
}

impl<E: Event> Widget for Slider<E> {
    type State = Sliding;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    /// The width it is offered, or ten ems with none, and one em down, at its style's font size.
    fn measure(&self, style: &Style, measuring: &mut Measuring<'_>, offer: Offer) -> Vec2 {
        let em = style.font_size * measuring.scale();
        let width = match offer.width {
            Offered::Exactly(width) | Offered::AtMost(width) => width,
            Offered::MinContent | Offered::MaxContent => em * WIDTH_EMS,
        };

        Vec2::new(width, em)
    }

    fn draw(&self, sliding: &Sliding, style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        let track = laid.content;
        let faint = Colour { alpha: style.colour.alpha * TRACK_STRENGTH, ..style.colour };
        let reach = track.width().mul_add(self.fraction(), track.min.x);
        let filled = Rect2 { min: track.min, max: Vec2::new(reach, track.max.y) };
        let bar = |rect: Rect2, colour: Colour| Quad {
            rect,
            clip: laid.clip,
            colour,
            border_width: 0.0,
            border_colour: colour,
            radius: track.height() / 2.0,
            sample: Sample::Solid,
        };
        draws.quad(bar(track, faint));
        draws.quad(bar(filled, style.colour));
        let lively = sliding.hovered || sliding.dragging;
        let grown = if lively {
            GROWTH * laid.scale
        } else {
            0.0
        };
        let side = track.height() + grown * 2.0;
        let middle = Vec2::new(filled.max.x, track.min.y + track.height() / 2.0);
        let knob = Rect2::from_origin_size(middle - Vec2::new(side / 2.0, side / 2.0), Vec2::new(side, side));
        draws.quad(Quad { radius: side / 2.0, ..bar(knob, Colour::WHITE) });
        ring(style, laid, draws);
    }

    fn pointer(&self, sliding: &Sliding) -> Pointer {
        let cursor = if sliding.dragging {
            Cursor::Grabbing
        } else {
            Cursor::Grab
        };

        Pointer::Blocks(cursor)
    }

    fn focusable(&self) -> bool {
        true
    }

    fn hears(&self) -> Hears {
        Hears::Keys(STEPS)
    }

    fn pointed(&self, sliding: &mut Sliding, pointing: Pointing, input: &mut Input<'_, '_>) {
        match pointing {
            Pointing::Entered => sliding.hovered = true,
            Pointing::Left => sliding.hovered = false,
            Pointing::Down { at, button: Button::Main } => {
                sliding.dragging = true;
                self.dragged(at, input);
            }
            Pointing::Moved { at } if sliding.dragging => self.dragged(at, input),
            Pointing::Up { button: Button::Main, .. } => sliding.dragging = false,
            Pointing::Moved { .. } | Pointing::Down { .. } | Pointing::Up { .. } => {}
        }
    }

    fn key(&self, _: &mut Sliding, key: &Key, input: &mut Input<'_, '_>) -> Used {
        let (least, most) = self.ends();
        let stride = self.stride();
        let value = match key.code.as_str() {
            "ArrowLeft" | "ArrowDown" => self.value - stride,
            "ArrowRight" | "ArrowUp" => self.value + stride,
            "PageDown" => stride.mul_add(-PAGE, self.value),
            "PageUp" => stride.mul_add(PAGE, self.value),
            "Home" => least,
            "End" => most,
            _ => return Used::Passed,
        };
        self.set(value, input);

        Used::Used
    }
}
