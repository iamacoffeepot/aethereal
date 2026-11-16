//! `Picture`: an image from pixels the caller decoded, drawn from a texture its screen makes of them (ADR-0056:
//! Pictures). Its content is the image's size in logical pixels, a pixel to each, shrunk to the width it is offered
//! with its height in proportion; a box its style sizes otherwise shows the whole image within it, its proportions
//! kept, centred. It is never shorter than [`Picture::LEAST`], or its image's own height when that is less, so a transcript
//! of pictures alone still bounds how many fill its view.
//!
//! The screen makes the texture the first time a layout meets the picture, waiting a frame for it as it waits for a
//! font's metrics, and releases it once no widget shows it.

#[cfg(test)]
mod tests;

use crate::fonts::Measuring;
use crate::pixels::Pixels;
use crate::style::Style;
use crate::widget::{Arrange, Draws, Laid, Offer, Offered, Quad, Sample, Slices, Stateless, Widget};
use aethereal_math::{Rect2, Vec2};

/// The image `pixels`, tinted by its style's colour.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    pub pixels: Pixels,
}

impl Picture {
    /// The least height a picture takes, in logical pixels, unless its image is shorter.
    pub const LEAST: f32 = 16.0;

    /// Its image's size, in pixels at `scale`.
    fn natural(&self, scale: f32) -> Vec2 {
        let (width, height) = self.pixels.size();
        // A picture's sides are at most 8192, which an f32 holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let size = Vec2::new(width as f32, height as f32);

        size * scale
    }

    /// The least height it takes at `scale`.
    fn least_height(&self, scale: f32) -> f32 {
        self.natural(scale).y.min(Self::LEAST * scale)
    }
}

/// `natural`, scaled to fit within `room`, its proportions kept: never enlarged.
fn fitted(natural: Vec2, room: Vec2) -> Vec2 {
    let across = room.x / natural.x.max(f32::MIN_POSITIVE);
    let down = room.y / natural.y.max(f32::MIN_POSITIVE);
    let shrink = across.min(down).clamp(0.0, 1.0);

    natural * shrink
}

impl Widget for Picture {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    /// Its image's size, shrunk to the width offered with its height in proportion, never shorter than its least.
    fn measure(&self, _: &Style, measuring: &mut Measuring<'_>, offer: Offer) -> Vec2 {
        let scale = measuring.scale();
        let natural = self.natural(scale);
        let width = match offer.width {
            Offered::Exactly(width) | Offered::AtMost(width) => width.min(natural.x),
            Offered::MinContent => natural.x.min(Self::LEAST * scale),
            Offered::MaxContent => natural.x,
        };
        let fitted = fitted(natural, Vec2::new(width.max(0.0), natural.y));

        Vec2::new(fitted.x, fitted.y.max(self.least_height(scale)))
    }

    fn least(&self, _: &Style, measuring: &mut Measuring<'_>) -> Vec2 {
        Vec2::new(0.0, self.least_height(measuring.scale()))
    }

    /// The whole image, within its content box, its proportions kept, centred.
    fn draw(&self, _: &Stateless, style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        let content = laid.content;
        let shown = fitted(self.natural(laid.scale), content.size());
        let at = content.min + (content.size() - shown) * 0.5;
        draws.quad(Quad {
            rect: Rect2 { min: at, max: at + shown },
            clip: laid.clip,
            colour: style.colour,
            border_width: 0.0,
            border_colour: style.colour,
            radius: style.radius * laid.scale,
            sample: Sample::Picture {
                picture: self.pixels.id(),
                uv: Rect2 { min: Vec2::ZERO, max: Vec2::new(1.0, 1.0) },
                slices: Slices::NONE,
            },
        });
    }
}
