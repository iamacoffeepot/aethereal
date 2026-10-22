//! What the chassis serves the renderer each frame: each canvas's surface, which it configures to its window's size,
//! acquires a frame of, and hands the renderer that frame's texture through its front before the cycle, then presents
//! once the renderer asks; and the buffers it asks to have mapped for reading, mapped after the cycle, and word of each
//! pushed onto its source once the GPU has (ADR-0019).

use aethereal_actor::Requests;
use aethereal_render::mapping::{Mapped, Mapping};
use aethereal_render::surface::{Present, WindowSurface};
use aethereal_substrate::Feed;
use core::cell::Cell;
use std::rc::Rc;

/// What the chassis serves the engine's renderer each frame, if it has one.
pub enum Serving {
    /// The renderer's: it draws on the canvases.
    Drawing(Box<Drawing>),
    /// Nothing: the engine has no renderer, as a page that draws nothing has.
    Nothing,
}

/// What the chassis keeps for the renderer: each canvas's surface, in the order the canvases were opened, its end of
/// the renderer's controller for presenting each canvas's frame, and its ends of the renderer's controller and source
/// for the buffers it maps.
pub struct Drawing {
    pub(crate) surfaces: Vec<Surfaced>,
    pub(crate) presents: Requests<Present>,
    pub(crate) requests: Requests<Mapping>,
    pub(crate) feed: Feed<Mapped>,
}

/// A canvas's surface, and its window's size as last pushed, which the surface is configured to.
pub struct Surfaced {
    pub(crate) surface: WindowSurface,
    pub(crate) size: Rc<Cell<(u32, u32)>>,
}

impl Serving {
    /// Before the cycle: configures each canvas's surface to its window's size as last pushed when it is not
    /// configured to it, acquires its frame, and hands the renderer the frame's texture, which in a browser does not
    /// wait.
    pub(crate) fn acquire(&mut self) {
        let Self::Drawing(drawing) = self else {
            return;
        };
        for surfaced in &mut drawing.surfaces {
            let (width, height) = surfaced.size.get();
            surfaced.surface.resized(width, height);
            surfaced.surface.acquire();
        }
    }

    /// After the cycle: presents each canvas's frame the renderer drew, once it asks, and starts mapping each buffer it
    /// asked for since the last frame. The page's promise pushes what became of each when the GPU has mapped it, and
    /// nothing waits for it.
    pub(crate) fn serve(&mut self) {
        let Self::Drawing(drawing) = self else {
            return;
        };
        let asked = drawing.presents.take();
        for surfaced in &mut drawing.surfaces {
            surfaced.surface.present(&asked);
        }
        for mapping in drawing.requests.take() {
            let feed = drawing.feed.clone();
            mapping.start(move |mapped| {
                // Refused only once the renderer has gone, and nothing is then owed what it read.
                let _ = feed.push(mapped);
            });
        }
    }
}
