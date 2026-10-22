//! What the chassis serves the renderer each frame: each window's surface, which it configures to the window's size,
//! acquires a frame of, and hands the renderer that frame's texture through its front before the cycle, then presents
//! once the renderer asks; and the buffers it asks to have mapped for reading, mapped after the cycle, and word of each
//! pushed onto its source once the device, polled without waiting, has mapped it (ADR-0019, ADR-0021). Between steps
//! it opens and closes the windows the window root asks for (ADR-0027).

use crate::gpu::{self, Unopened};
use aethereal_actor::Requests;
use aethereal_render::Gpu;
use aethereal_render::mapping::{Mapped, Mapping};
use aethereal_render::surface::{Fronting, Present, Surfaced, WindowSurface};
use aethereal_substrate::Feed;
use aethereal_window::{self as window, Hosting, Logical, WindowFeed, WindowId};
use std::collections::HashMap;
use std::sync::Arc;
use winit::dpi::LogicalSize;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId as NativeId};

/// What the chassis keeps for the renderer and the windows: each window, in the order opened; winit's id for each
/// beside the window capability's, which routes what winit says of a window to it (ADR-0024); its end of the
/// renderer's controller for presenting each window's frame; its ends of the renderer's controller and source for the
/// buffers it maps; and what it opens a window with while the program runs: the instance, the adapter, and the GPU
/// the renderer draws with, whose device it polls, how each surface is presented, and its end of the renderer's front
/// and of the source it tells the renderer of each window through (ADR-0027). The end the renderer's bootstrap gives
/// the desktop (ADR-0029).
pub struct Drawing {
    pub(crate) windows: Vec<Kept>,
    pub(crate) routes: HashMap<NativeId, WindowId>,
    pub(crate) presents: Requests<Present>,
    pub(crate) requests: Requests<Mapping>,
    pub(crate) mappings: Feed<Mapped>,
    pub(crate) instance: wgpu::Instance,
    pub(crate) adapter: wgpu::Adapter,
    pub(crate) gpu: Gpu,
    pub(crate) present: wgpu::PresentMode,
    pub(crate) fronting: Fronting,
    pub(crate) surfacing: Feed<Surfaced>,
}

/// A window the chassis opened: the native window, the feed of what it hears, its surface, where the pointer was last
/// heard over it, in its surface pixels, its top left until it is heard, and whether it is shown.
pub struct Kept {
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

impl Kept {
    /// The window `native`, fed through `feed`, drawn to through `surface`: shown, the pointer at its top left.
    pub(crate) const fn new(native: Arc<Window>, feed: WindowFeed, surface: WindowSurface) -> Self {
        Self { native, feed, surface, pointer: (0.0, 0.0), visible: Visible::Shown }
    }

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
    pub(crate) fn window_mut(&mut self, window: WindowId) -> Option<&mut Kept> {
        self.windows.iter_mut().find(|opened| opened.feed.window() == window)
    }

    /// Before the cycle: acquires the frame of each window drawn this frame, shown and at a size, in turn, each surface
    /// configured first to the size its window was last resized to, and hands the renderer each frame's texture. With
    /// `Fifo`, the renderer's default, each waits until the display frees an image, on the event loop's thread, which
    /// is what paces the frames: a window on a slower display paces every window to its rate. Answers whether any
    /// window is drawn.
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
        let _ = self.gpu.device().poll(wgpu::PollType::Poll);
    }

    /// Between steps, inside one of `event_loop`'s callbacks, where a native window is made: opens `window`, as the
    /// window root asked, titled `title`, at `size` in logical pixels. It makes the native window, its surface on the
    /// renderer's GPU, and the window's slot in the renderer's front, then tells the renderer of it, then the window
    /// root through `hosting`, which so hears of it in the same bring and answers its asker a step after, so the
    /// renderer knows the window before any actor can name it. A window or surface not made is answered `Failed`, with
    /// why.
    pub(crate) fn open(
        &mut self,
        event_loop: &ActiveEventLoop,
        hosting: &Hosting,
        window: WindowId,
        title: &str,
        size: Logical,
    ) {
        let made = self.made(event_loop, window, title, size);
        let (native, surface) = match made {
            Ok(made) => made,
            Err(unopened) => {
                // Refused only once the window root has gone, and no one is then owed an answer.
                let _ = hosting.unopened(window, window::Unopened::Failed(unopened.to_string()));
                return;
            }
        };
        let format = surface.drawn().format;
        // Refused only once the renderer has gone, and nothing is then drawn.
        let _ = self.surfacing.push(Surfaced::Opened { window, format });
        let Ok(feed) = hosting.opened(window, gpu::standing(&native), native.has_focus()) else {
            self.fronting.remove(window);
            let _ = self.surfacing.push(Surfaced::Closed { window });
            return;
        };
        self.routes.insert(native.id(), window);
        self.windows.push(Kept::new(native, feed, surface));
    }

    /// The native window `window`, titled `title`, at `size` in logical pixels, and its surface, its slot added to
    /// the renderer's front.
    fn made(
        &self,
        event_loop: &ActiveEventLoop,
        window: WindowId,
        title: &str,
        size: Logical,
    ) -> Result<(Arc<Window>, WindowSurface), Unopened> {
        let native = gpu::native(event_loop, title, LogicalSize::new(size.width, size.height))?;
        let surface = self.instance.create_surface(Arc::clone(&native)).map_err(Unopened::Surface)?;
        let config = gpu::configured(&self.adapter, &surface, &native, self.present)?;
        let surface =
            WindowSurface::new(surface, &self.gpu, window, config, &self.fronting).map_err(Unopened::Format)?;

        Ok((native, surface))
    }

    /// Between steps: closes `window`, as the window root asked or as its host did. It drops the window's surface,
    /// takes its slot out of the renderer's front, and closes the native window, then tells the renderer, then the
    /// window root, through `hosting`. A window not open is told closed all the same, as it is not open.
    pub(crate) fn close(&mut self, hosting: &Hosting, window: WindowId) {
        let at = self.windows.iter().position(|opened| opened.feed.window() == window);
        if let Some(at) = at {
            let Kept { native, surface, .. } = self.windows.remove(at);
            self.routes.remove(&native.id());
            drop(surface);
            self.fronting.remove(window);
            drop(native);
        }
        // Each refused only once its root has gone, and nothing then hears of it.
        let _ = self.surfacing.push(Surfaced::Closed { window });
        let _ = hosting.closed(window);
    }
}
