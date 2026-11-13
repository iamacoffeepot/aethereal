//! A colour as a style writes it (ADR-0056: Styles are Rust items): its sRGB channels from 0 to 255, as a colour picker
//! shows them, turned into the linear light the renderer's colours are in.

use aethereal_render::text::Colour;

/// The opaque colour whose sRGB channels are `red`, `green`, and `blue`, each from 0 to 255: what a style's
/// `rgb(232, 217, 160)` names.
#[must_use]
pub fn rgb(red: u8, green: u8, blue: u8) -> Colour {
    rgba(red, green, blue, u8::MAX)
}

/// The colour whose sRGB channels are `red`, `green`, and `blue`, and whose alpha is `alpha`, each from 0 to 255: what a
/// style's `rgba(…)`, or its `"#rrggbbaa"`, names. Alpha is a coverage, not a light, so it is not decoded.
#[must_use]
pub fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Colour {
    Colour { red: linear(red), green: linear(green), blue: linear(blue), alpha: f32::from(alpha) / 255.0 }
}

/// An sRGB channel from 0 to 255, in linear light from 0 to 1: the sRGB transfer function undone.
fn linear(channel: u8) -> f32 {
    let encoded = f32::from(channel) / 255.0;
    let dark = encoded <= 0.040_45;
    if dark {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}
