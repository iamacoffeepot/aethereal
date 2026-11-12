//! `Text`: a string in its style's font, size, and colour, wrapped to the width it is given (ADR-0056: Consequences).
//!
//! It measures with its font's metrics, as the renderer lays it out, and draws in exactly the box it measured: a text
//! that fits unbroken is made unwrapped, and one that does not is made to wrap at its measured width, its widest line,
//! so its lines break where they were measured to. A word too long to break measures wider than the width offered; such
//! a text wraps at the offered width, and its box cuts the word. A rim around its glyphs draws outside them, so its box
//! is padded by the rim's width on every side.

#[cfg(test)]
mod tests;

use crate::fonts::{Measuring, Unmeasured};
use crate::style::Style;
use crate::widget::{Arrange, Draws, Laid, Offer, Offered, Stateless, TextDraw, Widget};
use aethereal_math::Vec2;
use aethereal_render::text::{Align, Measured, Outline, Run, Wrap};

/// A string, drawn in its style's font, size, and colour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    pub text: String,
}

impl Text {
    /// The string `text`.
    #[must_use]
    pub fn new(text: &str) -> Self {
        Self { text: String::from(text) }
    }

    /// Its runs: the string in the style's colour.
    fn runs(&self, style: &Style) -> Vec<Run> {
        vec![Run::new(&self.text, style.colour)]
    }

    /// Its box, as `measuring` lays it out at its style's size within `width` pixels, `None` for as wide as it wants,
    /// with the wrap it is made with to be drawn in that box.
    fn fitted(
        &self,
        style: &Style,
        measuring: &mut Measuring<'_>,
        width: Width,
    ) -> Result<(Measured, Wrap), Unmeasured> {
        let size = style.font_size * measuring.scale();
        let runs = self.runs(style);
        let mut measured = |wrap| measuring.text(style.font, size, &runs, wrap);
        let most = match width {
            Width::Unbounded => return measured(Wrap::None).map(|unbroken| (unbroken, Wrap::None)),
            Width::Narrowest => {
                let narrowest = measured(Wrap::At { width: f32::MIN_POSITIVE })?;
                return Ok((narrowest, Wrap::At { width: narrowest.width.max(f32::MIN_POSITIVE) }));
            }
            Width::Within(most) => most.max(0.0),
        };
        let unbroken = measured(Wrap::None)?;
        let fits = unbroken.width <= most;
        if fits {
            return Ok((unbroken, Wrap::None));
        }
        let offered = most.max(f32::MIN_POSITIVE);
        let wrapped = measured(Wrap::At { width: offered })?;
        let unbreakable = wrapped.width > offered;
        let wrap = if unbreakable {
            offered
        } else {
            wrapped.width.max(f32::MIN_POSITIVE)
        };

        Ok((wrapped, Wrap::At { width: wrap }))
    }
}

/// The width a text is laid out within.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Width {
    /// As wide as it wants: its lines broken only where it breaks them.
    Unbounded,
    /// As narrow as it goes: its widest word.
    Narrowest,
    /// Up to this many pixels.
    Within(f32),
}

/// How wide a text's rim is, in pixels at `scale`: none for a text with no rim.
fn rim(style: &Style, scale: f32) -> f32 {
    match style.outline {
        Outline::None => 0.0,
        Outline::Of { width, .. } => width * scale,
    }
}

impl Widget for Text {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    fn measure(&self, style: &Style, measuring: &mut Measuring<'_>, offer: Offer) -> Vec2 {
        let pad = rim(style, measuring.scale()) * 2.0;
        let width = match offer.width {
            Offered::Exactly(width) | Offered::AtMost(width) => Width::Within(width - pad),
            Offered::MinContent => Width::Narrowest,
            Offered::MaxContent => Width::Unbounded,
        };
        let Ok((measured, _)) = self.fitted(style, measuring, width) else {
            return Vec2::ZERO;
        };

        Vec2::new(measured.width + pad, measured.height + pad)
    }

    fn draw(&self, _: &Stateless, style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        let pad = rim(style, laid.scale);
        let width = pad.mul_add(-2.0, laid.content.width());
        let Ok((measured, wrap)) = self.fitted(style, draws.measuring(), Width::Within(width)) else {
            return;
        };
        let set = match wrap {
            Wrap::None => measured.width,
            Wrap::At { width } => width,
        };
        let shift = match style.text_align {
            Align::Left => 0.0,
            Align::Centre => (width - set) / 2.0,
            Align::Right => width - set,
        };
        draws.text(TextDraw {
            at: laid.content.min + Vec2::new(pad + shift, pad),
            clip: laid.clip.intersection(laid.content),
            font: style.font,
            size: style.font_size * laid.scale,
            wrap,
            align: style.text_align,
            outline: scaled(style.outline, laid.scale),
            runs: self.runs(style),
        });
    }
}

/// `outline`, its width at `scale`.
fn scaled(outline: Outline, scale: f32) -> Outline {
    match outline {
        Outline::None => Outline::None,
        Outline::Of { width, colour } => Outline::Of { width: width * scale, colour },
    }
}
