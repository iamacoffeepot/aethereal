//! What the chassis serves the renderer each frame: each window's surface, which it configures to the window's size,
//! acquires a frame of, and hands the renderer that frame's texture through its front before the cycle, then presents
//! once the renderer asks; and the buffers it asks to have mapped for reading, mapped after the cycle, and word of each
//! pushed onto its source once the device, polled without waiting, has mapped it (ADR-0019, ADR-0021).

use aethereal_actor::Requests;
use aethereal_render::mapping::{Mapped, Mapping};
use aethereal_render::surface::{Present, WindowSurface};
use aethereal_substrate::Feed;
use aethereal_window::{WindowFeed, WindowId};
use std::collections::HashMap;
use std::sync::Arc;
use winit::window::{Window, WindowId as NativeId};

/// What the chassis keeps for the renderer and the windows: each window, in the order the program opened them; winit's
/// id for each beside the window capability's, which routes what winit says of a window to it (ADR-0024); its end of
/// the renderer's controller for presenting each window's frame; the device that draws to them, which the chassis
/// polls; and its ends of the renderer's controller and source for the buffers it maps. Made by
/// [`renderer`](crate::renderer).
pub struct Drawing {
    pub(crate) windows: Vec<Opened>,
    pub(crate) routes: HashMap<NativeId, WindowId>,
    pub(crate) presents: Requests<Present>,
    pub(crate) device: wgpu::Device,
    pub(crate) requests: Requests<Mapping>,
    pub(crate) mappings: Feed<Mapped>,
}

/// A window the chassis opened: the native window, the feed of what it hears, its surface, where the pointer was last
/// heard over it, in its surface pixels, its top left until it is heard, and whether it is shown.
pub struct Opened {
    pub(crate) native: Arc<Window>,
    pub(crate) feed: WindowFeed,
    pub(crate) surface: WindowSurface,
    pub(crate) pointer: (f32, f32),
    pub(crate) visible: Visible,
}

/// Whether a window is shown, as winit last said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visible {
    /// It is, at least in part.
    Shown,
    /// It is hidden, minimised, or covered whole: it is handed no frame.
    Occluded,
}

impl Opened {
    /// Whether it is drawn this frame: shown, at a size.
    fn draws(&self) -> bool {
        let shown = self.visible == Visible::Shown;
        let size = self.native.inner_size();
        let sized = size.width > 0 && size.height > 0;

        shown && sized
    }
}

impl Drawing {
    /// The window the window capability calls `window`: none when the chassis opened none it calls so.
    pub(crate) fn window_mut(&mut self, window: WindowId) -> Option<&mut Opened> {
        self.windows.iter_mut().find(|opened| opened.feed.window() == window)
    }

    /// Before the cycle: acquires the frame of each window drawn this frame, shown and at a size, in turn, each surface
    /// configured first to the size its window was last resized to, and hands the renderer each frame's texture. With
    /// `Fifo`, each waits until the display frees an image, on the event loop's thread, which is what paces the
    /// frames: a window on a slower display paces every window to its rate. Answers whether any window is drawn.
    pub(crate) fn acquire(&mut self) -> bool {
        let mut drawn = false;
        for opened in &mut self.windows {
            if opened.draws() {
                opened.surface.acquire();
                drawn = true;
            }
        }

        drawn
    }

    /// After the cycle: presents each window's frame the renderer drew, once it asks, and starts mapping each buffer it
    /// asked for since the last frame.
    pub(crate) fn serve(&mut self) {
        let asked = self.presents.take();
        for opened in &mut self.windows {
            opened.surface.present(&asked);
        }
        for mapping in self.requests.take() {
            let mappings = self.mappings.clone();
            mapping.start(move |mapped| {
                // Refused only once the renderer has gone, and nothing is then owed what it read.
                let _ = mappings.push(mapped);
            });
        }
    }

    /// Asks for the next frame of the first window drawn, which paces them all: one frame draws every window drawn,
    /// so asking each would draw a frame for each.
    pub(crate) fn ask_next(&self) {
        let pacing = self.windows.iter().find(|opened| opened.draws());
        if let Some(opened) = pacing {
            opened.native.request_redraw();
        }
    }

    /// Polls the device without waiting, between steps: it calls back for each buffer the GPU has mapped by then, and
    /// what became of it is pushed onto the renderer's source.
    pub(crate) fn poll(&self) {
        // A device lost is the renderer's to hear of, as it next draws; polling only calls back what is done.
        let _ = self.device.poll(wgpu::PollType::Poll);
    }
}
