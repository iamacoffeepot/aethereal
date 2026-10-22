//! What the chassis serves the renderer each frame: the canvas's surface, which it configures to the window's size,
//! acquires a frame of, and hands the renderer that frame's texture through its front before the cycle, then presents
//! once the renderer asks; and the buffers it asks to have mapped for reading, mapped after the cycle, and word of each
//! pushed onto its source once the GPU has (ADR-0019).

use aethereal_actor::Requests;
use aethereal_render::mapping::{Mapped, Mapping};
use aethereal_render::surface::WindowSurface;
use aethereal_substrate::Feed;
use core::cell::Cell;
use std::rc::Rc;

/// What the chassis serves the engine's renderer each frame, if it has one.
pub enum Serving {
    /// The renderer's: it draws on the canvas.
    Drawing(Box<Drawing>),
    /// Nothing: the engine has no renderer, as a page that draws nothing has.
    Nothing,
}

/// What the chassis keeps for the renderer: the canvas's surface, the window's size as last pushed, which the surface
/// is configured to, and its ends of the renderer's controller and source for the buffers it maps.
pub struct Drawing {
    pub(crate) surface: WindowSurface,
    pub(crate) size: Rc<Cell<(u32, u32)>>,
    pub(crate) requests: Requests<Mapping>,
    pub(crate) feed: Feed<Mapped>,
}

impl Serving {
    /// Before the cycle: configures the canvas's surface to the window's size as last pushed when it is not
    /// configured to it, acquires its frame, and hands the renderer the frame's texture, which in a browser does not
    /// wait.
    pub(crate) fn acquire(&mut self) {
        let Self::Drawing(drawing) = self else {
            return;
        };
        let (width, height) = drawing.size.get();
        drawing.surface.resized(width, height);
        drawing.surface.acquire();
    }

    /// After the cycle: presents the frame the renderer drew, once it asks, and starts mapping each buffer it asked
    /// for since the last frame. The page's promise pushes what became of each when the GPU has mapped it, and nothing
    /// waits for it.
    pub(crate) fn serve(&mut self) {
        let Self::Drawing(drawing) = self else {
            return;
        };
        drawing.surface.present();
        for mapping in drawing.requests.take() {
            let feed = drawing.feed.clone();
            mapping.start(move |mapped| {
                // Refused only once the renderer has gone, and nothing is then owed what it read.
                let _ = feed.push(mapped);
            });
        }
    }
}
