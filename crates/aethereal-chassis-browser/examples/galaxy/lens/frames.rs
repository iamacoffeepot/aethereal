//! The galaxy's two frames. The glow's ends on a texture smaller than the window: it moves the stars, draws them
//! small, keeps their bright part, and blurs it across and down. The window's draws the stars at its full size and lays
//! the glow's texture over them. The window's frame reads the glow's texture, so the glow's runs first in each
//! `Render`, and the stars it moved are where both frames draw them.

use super::bloom::{Effects, composited};
use crate::passes::{Across, Bright, Composite, Down, Simulate, Small, Stars};
use aethereal_render::frame::builder::{Effect, Float, FrameBuilder, depth};
use aethereal_render::frame::{CreateFrame, Ends, FrameRefused, Size};
use aethereal_render::texture::TextureId;
use aethereal_window::WindowId;

/// How many times smaller the glow is than the window, along each side.
const SMALLER: u32 = 2;

/// The size of the glow for a window of `width` by `height` pixels.
pub fn glow_size(width: u32, height: u32) -> Size {
    Size { width: (width / SMALLER).max(1), height: (height / SMALLER).max(1) }
}

/// The glow's frame, of `size`, its bloom drawn by `effects`.
pub fn glow(size: Size, effects: &Effects) -> Result<CreateFrame, FrameRefused> {
    let mut frame = FrameBuilder::on(Ends::Texture(size));
    frame.compute(Simulate);
    let small = frame.draw(Small, Float, depth::Off);
    let bright = frame.effect(Bright, Effect::new(&effects.bright, (small,)), Float);
    let across = frame.effect(Across, Effect::new(&effects.across, (bright,)), Float);

    frame.finish(Down, Effect::new(&effects.down, (across,)))
}

/// The window's frame, on `window`, laying the glow's texture `glow` over the stars by `effects`' composite.
pub fn shown(window: WindowId, effects: &Effects, glow: TextureId) -> Result<CreateFrame, FrameRefused> {
    let mut frame = FrameBuilder::on(Ends::Window(window));
    let stars = frame.draw(Stars, Float, depth::Off);

    frame.finish(Composite, Effect::new(&effects.composite, (stars,)).bound(composited(glow)))
}
