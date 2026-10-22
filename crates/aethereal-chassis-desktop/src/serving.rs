//! What the chassis serves the renderer each frame: the window's surface, which it configures to the window's size,
//! acquires a frame of, and hands the renderer that frame's texture through its front before the cycle, then presents
//! once the renderer asks; and the buffers it asks to have mapped for reading, mapped after the cycle, and word of each
//! pushed onto its source once the device, polled without waiting, has mapped it (ADR-0019, ADR-0021).

use aethereal_actor::Requests;
use aethereal_render::mapping::{Mapped, Mapping};
use aethereal_render::surface::WindowSurface;
use aethereal_substrate::Feed;
use aethereal_window::WindowFeed;
use std::sync::Arc;
use winit::window::Window;

/// What the chassis keeps for the renderer and the one window: the native window, the feed of what it hears, its
/// surface, the device that draws to it, which the chassis polls, and its ends of the renderer's controller and source
/// for the buffers it maps. Made by [`renderer`](crate::renderer).
pub struct Drawing {
    pub(crate) window: Arc<Window>,
    pub(crate) feed: WindowFeed,
    pub(crate) surface: WindowSurface,
    pub(crate) device: wgpu::Device,
    pub(crate) requests: Requests<Mapping>,
    pub(crate) mappings: Feed<Mapped>,
}

impl Drawing {
    /// Before the cycle: acquires the surface's frame, configured first to the size the window was last resized to,
    /// and hands the renderer the frame's texture. With `Fifo`, this waits until the display frees an image, on the
    /// event loop's thread, which is what paces the frames.
    pub(crate) fn acquire(&mut self) {
        self.surface.acquire();
    }

    /// After the cycle: presents the frame the renderer drew, once it asks, and starts mapping each buffer it asked for
    /// since the last frame.
    pub(crate) fn serve(&mut self) {
        self.surface.present();
        for mapping in self.requests.take() {
            let mappings = self.mappings.clone();
            mapping.start(move |mapped| {
                // Refused only once the renderer has gone, and nothing is then owed what it read.
                let _ = mappings.push(mapped);
            });
        }
    }

    /// Polls the device without waiting, between steps: it calls back for each buffer the GPU has mapped by then, and
    /// what became of it is pushed onto the renderer's source.
    pub(crate) fn poll(&self) {
        // A device lost is the renderer's to hear of, as it next draws; polling only calls back what is done.
        let _ = self.device.poll(wgpu::PollType::Poll);
    }
}
