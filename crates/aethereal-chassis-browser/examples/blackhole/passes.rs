//! The passes of the black hole's two frames, named by types the lens that makes the frames and the placement of the
//! photons' quad share.

use aethereal_data::{Kind, KindName, Plain};
use aethereal_render::pass::Pass;

/// The glow's frame's pass that traces the photons small, into a target of floats at the glow's size.
#[derive(Plain)]
pub struct Small;

impl Kind for Small {
    const NAME: KindName = KindName::new("aether.example.blackhole.small");
}

impl Pass for Small {}

/// The glow's frame's pass that keeps the bright part of what [`Small`] traced.
#[derive(Plain)]
pub struct Bright;

impl Kind for Bright {
    const NAME: KindName = KindName::new("aether.example.blackhole.bright");
}

impl Pass for Bright {}

/// The glow's frame's pass that blurs the bright part along each row.
#[derive(Plain)]
pub struct Across;

impl Kind for Across {
    const NAME: KindName = KindName::new("aether.example.blackhole.across");
}

impl Pass for Across {}

/// The glow's frame's last pass, which blurs along each column onto the frame's end.
#[derive(Plain)]
pub struct Down;

impl Kind for Down {
    const NAME: KindName = KindName::new("aether.example.blackhole.down");
}

impl Pass for Down {}

/// The window's frame's pass that traces the photons, into a target of floats at the window's size.
#[derive(Plain)]
pub struct Trace;

impl Kind for Trace {
    const NAME: KindName = KindName::new("aether.example.blackhole.trace");
}

impl Pass for Trace {}

/// The window's frame's last pass, which lays the glow over the light and maps it onto the window.
#[derive(Plain)]
pub struct Composite;

impl Kind for Composite {
    const NAME: KindName = KindName::new("aether.example.blackhole.composite");
}

impl Pass for Composite {}
