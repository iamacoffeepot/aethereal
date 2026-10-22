//! The passes of the galaxy's two frames, named by types the lens that makes the frames and the disc that dispatches
//! into one share.

use aethereal_data::{Kind, KindName, Plain};
use aethereal_render::pass::Pass;

/// The glow's frame's compute pass, which moves the stars: it runs first in the `Render`, so both frames draw where it
/// left them.
#[derive(Plain)]
pub struct Simulate;

impl Kind for Simulate {
    const NAME: KindName = KindName::new("aether.example.galaxy.simulate");
}

impl Pass for Simulate {}

/// The glow's frame's pass that draws the stars small, into a target of floats at the glow's size.
#[derive(Plain)]
pub struct Small;

impl Kind for Small {
    const NAME: KindName = KindName::new("aether.example.galaxy.small");
}

impl Pass for Small {}

/// The glow's frame's pass that keeps the bright part of what [`Small`] drew.
#[derive(Plain)]
pub struct Bright;

impl Kind for Bright {
    const NAME: KindName = KindName::new("aether.example.galaxy.bright");
}

impl Pass for Bright {}

/// The glow's frame's pass that blurs the bright part along each row.
#[derive(Plain)]
pub struct Across;

impl Kind for Across {
    const NAME: KindName = KindName::new("aether.example.galaxy.across");
}

impl Pass for Across {}

/// The glow's frame's last pass, which blurs along each column onto the frame's end.
#[derive(Plain)]
pub struct Down;

impl Kind for Down {
    const NAME: KindName = KindName::new("aether.example.galaxy.down");
}

impl Pass for Down {}

/// The window's frame's pass that draws the stars, into a target of floats at the window's size.
#[derive(Plain)]
pub struct Stars;

impl Kind for Stars {
    const NAME: KindName = KindName::new("aether.example.galaxy.stars");
}

impl Pass for Stars {}

/// The window's frame's last pass, which lays the glow over the stars onto the window.
#[derive(Plain)]
pub struct Composite;

impl Kind for Composite {
    const NAME: KindName = KindName::new("aether.example.galaxy.composite");
}

impl Pass for Composite {}
