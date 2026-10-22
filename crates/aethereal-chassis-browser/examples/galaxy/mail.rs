//! What the disc and the lens are made with, and the mail between them: the lens asks the disc to move the stars in a
//! frame once it is made, and the disc answers with the stars once they move there.

use aethereal_data::{Kind, KindName, Plain};
use aethereal_render::frame::FrameId;
use aethereal_render::storage::StorageId;
use aethereal_window::WindowId;

/// The context of an ask that needs none.
#[derive(Plain)]
pub struct Nothing;

impl Kind for Nothing {
    const NAME: KindName = KindName::new("aether.example.galaxy.nothing");
}

/// The config of both roots: the window the galaxy is shown on, and its surface's size in pixels as the page set it.
#[derive(Debug, Clone, Copy, Plain)]
pub struct Canvas {
    pub window: WindowId,
    pub width: u32,
    pub height: u32,
}

impl Kind for Canvas {
    const NAME: KindName = KindName::new("aether.example.galaxy.canvas");
}

/// Asks the disc to move the stars in the compute pass of `frame`, the glow's frame. Answered with [`Moving`].
#[derive(Debug, Clone, Copy, Plain)]
pub struct Move {
    pub frame: FrameId,
}

impl Kind for Move {
    const NAME: KindName = KindName::new("aether.example.galaxy.move");
}

/// The disc's answer to [`Move`]: the stars move, held in `stars`.
#[derive(Debug, Clone, Copy, Plain)]
pub struct Moving {
    pub stars: StorageId,
}

impl Kind for Moving {
    const NAME: KindName = KindName::new("aether.example.galaxy.moving");
}
