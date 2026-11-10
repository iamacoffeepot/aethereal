//! What the chassis serves the renderer each frame: each window's surface, which it configures to the window's size,
//! acquires a frame of, and hands the renderer that frame's texture through its front before the cycle, then presents
//! once the renderer asks; and the buffers it asks to have mapped for reading, mapped after the cycle, and word of each
//! pushed onto its source once the device, polled without waiting, has mapped it (ADR-0019, ADR-0021). Between steps
//! it opens and closes the windows the window root asks for (ADR-0027).

use crate::Presenting;
use crate::gpu::{self, Unopened};
use crate::input::Composition;
use crate::moding;
use crate::redrawing::{self, Asked, Drew, Redraw};
use aethereal_actor::Requests;
use aethereal_render::Gpu;
use aethereal_render::mapping::{Mapped, Mapping};
use aethereal_render::stats::FrameBudget;
use aethereal_render::surface::{Fronting, Present, Surfaced, WindowSurface};
use aethereal_substrate::Feed;
use aethereal_window::{self as window, Event, Hosting, Logical, Mode, Surface, WindowFeed, WindowId};
use core::mem;
use std::collections::HashMap;
use std::sync::Arc;
use winit::dpi::LogicalSize;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId as NativeId};

/// What the chassis keeps for the renderer and the windows: each window, in the order opened; winit's id for each
/// beside the window capability's, which routes what winit says of a window to it (ADR-0024); its end of the
/// renderer's controller for presenting each window's frame; its ends of the renderer's controller and source for the
/// buffers it maps; and what it opens a window with while the program runs: the instance, the adapter, and the GPU
/// the renderer draws with, whose device it polls, how each surface is presented, and its end of the renderer's front
/// and of the source it tells the renderer of each window through (ADR-0027), and the frame budget it last said to the
/// renderer through its front (ADR-0053). The end the renderer's bootstrap gives the desktop (ADR-0029).
pub struct Drawing {
    pub(crate) windows: Vec<Kept>,
    pub(crate) routes: HashMap<NativeId, WindowId>,
    pub(crate) presents: Requests<Present>,
    pub(crate) requests: Requests<Mapping>,
    pub(crate) mappings: Feed<Mapped>,
    pub(crate) instance: wgpu::Instance,
    pub(crate) adapter: wgpu::Adapter,
    pub(crate) gpu: Gpu,
    pub(crate) present: Presenting,
    pub(crate) fronting: Fronting,
    pub(crate) surfacing: Feed<Surfaced>,
    pub(crate) budget: FrameBudget,
}

/// A window the chassis opened: the native window, the feed of what it hears, its surface, where the pointer was last
/// heard over it, in its surface pixels, its top left until it is heard, whether it is shown, and the mode it is shown
/// in as last pushed onto its feed, the mode it opened in until another is; whether its next frame is drawn, as the
/// compositor last asked; the modifiers held in it as winit last said, none until it says; and whether its input method
/// has a composition open (ADR-0051).
pub struct Kept {
    pub(crate) native: Arc<Window>,
    pub(crate) feed: WindowFeed,
    pub(crate) surface: WindowSurface,
    pub(crate) pointer: (f32, f32),
    pub(crate) visible: Visible,
    pub(crate) mode: Mode,
    pub(crate) redraw: Redraw,
    pub(crate) modifiers: ModifiersState,
    pub(crate) composition: Composition,
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
    /// The window `native`, fed through `feed`, drawn to through `surface`, opened in `mode`: shown, the pointer at its
    /// top left, its first frame due, no modifier held, and no composition open, as its input method opens stopped.
    pub(crate) const fn new(native: Arc<Window>, feed: WindowFeed, surface: WindowSurface, mode: Mode) -> Self {
        Self {
            native,
            feed,
            surface,
            pointer: (0.0, 0.0),
            visible: Visible::Shown,
            mode,
            redraw: Redraw::Due,
            modifiers: ModifiersState::empty(),
            composition: Composition::Closed,
        }
    }

    /// Takes in that the window's inner size is now `width` by `height` of its pixels, as winit said with `Resized` or
    /// answered a size the program asked for with: configures its surface to it as the next frame is drawn, asks for
    /// that frame, and pushes the resize onto its feed, then the mode winit shows it in when that changed.
    pub(crate) fn resized(&mut self, width: u32, height: u32) {
        self.surface.resized(width, height);
        self.native.request_redraw();
        let surface = Surface { width, height, scale: self.native.scale_factor() };
        // Refused only once the window root has gone, and nothing then hears of it.
        let _ = self.feed.push(Event::Resized(surface));
        self.moded();
    }

    /// Reads the mode winit shows the window in, and pushes it onto its feed as `Moded` when it differs from the mode
    /// last pushed: as the window resizes, which it does whenever its mode changes, whether the program or the person at
    /// it changed it, and as the program sets it.
    pub(crate) fn moded(&mut self) {
        let mode = moding::shown(&self.native);
        let changed = mem::replace(&mut self.mode, mode) != mode;
        if changed {
            // Refused only once the window root has gone, and nothing then hears of it.
            let _ = self.feed.push(Event::Moded(mode));
        }
    }

    /// Whether it is drawn this frame: shown, at a size, and no frame of it held.
    fn draws(&self) -> bool {
        redrawing::drawn(self.visible, self.sized(), self.redraw)
    }

    /// Whether it shows: shown, at a size, its redraw due or not.
    fn shows(&self) -> bool {
        let shown = self.visible == Visible::Shown;

        shown && self.sized()
    }

    /// Whether its inner size has pixels.
    fn sized(&self) -> bool {
        let size = self.native.inner_size();

        size.width > 0 && size.height > 0
    }

    /// After a cycle: presents the frame the renderer drew of it, when it asked among `asked`, as `presenting` says,
    /// at once while its redraw is due, and otherwise holds it to present as the compositor asks for it. Just before a
    /// `fifo` present it tells winit, which then holds its next redraw until the compositor's frame callback for this
    /// frame, and asks for that redraw, which comes once the compositor does (ADR-0045). A window that held a frame
    /// from before was not drawn, and keeps it.
    fn present(&mut self, asked: &[Present], presenting: Presenting) {
        let holding = self.redraw == Redraw::Drawn;
        if holding {
            return;
        }
        match self.redraw.drew() {
            Drew::Hold => {
                let held = self.surface.hold(asked);
                if held {
                    self.redraw = self.redraw.held();
                }
            }
            Drew::Present => {
                let native = &self.native;
                let fifo = presenting == Presenting::Fifo;
                let notify = || {
                    if fifo {
                        native.pre_present_notify();
                    }
                };
                let presented = self.surface.present(asked, notify);
                if presented {
                    self.redraw = Redraw::presented(presenting);
                }
                let awaits = presented && fifo;
                if awaits {
                    native.request_redraw();
                }
            }
        }
    }

    /// Takes in that the compositor asked for its next frame, as winit's redraw of it says: presents the frame held,
    /// telling winit first, as `present` does, and then awaits the next ask; or, holding none, is due.
    fn redrawn(&mut self) {
        match self.redraw.asked() {
            Asked::PresentHeld => {
                let native = &self.native;
                let presented = self.surface.present_held(|| native.pre_present_notify());
                if presented {
                    self.redraw = Redraw::Awaiting;
                    native.request_redraw();
                } else {
                    self.redraw = Redraw::Due;
                }
            }
            Asked::Due => self.redraw = Redraw::Due,
        }
    }
}

impl Drawing {
    /// How each window's frames are presented, as the renderer's settings say.
    pub(crate) const fn presenting(&self) -> Presenting {
        self.present
    }

    /// The window the window capability calls `window`: none when the chassis opened none it calls so.
    pub(crate) fn window_mut(&mut self, window: WindowId) -> Option<&mut Kept> {
        self.windows.iter_mut().find(|opened| opened.feed.window() == window)
    }

    /// Before the cycle: acquires the frame of each window drawn this frame, shown, at a size, and holding no frame, in
    /// turn, each surface configured first to the size its window was last resized to, and hands the renderer each
    /// frame's texture. Presented `fifo`, the renderer's default, a window's frame is drawn one ahead and presented as
    /// the compositor asks for it, which is what paces the frames, and its acquire may wait until the display frees an
    /// image, on the event loop's thread. Presented `uncapped` or `capped`, none waits. Says the budget again first, when the first
    /// window's display changed or became known (ADR-0053). Answers whether any window is drawn.
    pub(crate) fn acquire(&mut self) -> bool {
        self.rebudget();
        let mut drawn = false;
        for opened in &mut self.windows {
            if opened.draws() {
                opened.surface.acquire();
                drawn = true;
            }
        }

        drawn
    }

    /// After the cycle: presents each window's frame the renderer drew, once it asks, or holds it while the window
    /// awaits the compositor's ask for it, and starts mapping each buffer it asked for since the last frame.
    pub(crate) fn serve(&mut self) {
        let asked = self.presents.take();
        for opened in &mut self.windows {
            opened.present(&asked, self.present);
        }
        self.map();
    }

    /// Takes in that the compositor asked for `window`'s next frame, as winit's redraw of it says: the frame held for
    /// it is presented, or it is drawn and presented next frame. A window not open is not drawn.
    pub(crate) fn redrawn(&mut self, window: WindowId) {
        if let Some(opened) = self.window_mut(window) {
            opened.redrawn();
        }
    }

    /// Whether a window that shows awaits the compositor's ask with no frame drawn ahead for it: one presented at once
    /// this frame, whose next is drawn ahead in a frame that follows at once.
    pub(crate) fn awaits_undrawn(&self) -> bool {
        self.showing().any(|redraw| redraw == Redraw::Awaiting)
    }

    /// Whether a window that shows holds a frame drawn ahead of the compositor's ask, which a held cycle waits on.
    pub(crate) fn holds(&self) -> bool {
        self.showing().any(|redraw| redraw == Redraw::Drawn)
    }

    /// The redraw of each window that shows.
    fn showing(&self) -> impl Iterator<Item = Redraw> + '_ {
        self.windows.iter().filter(|opened| opened.shows()).map(|opened| opened.redraw)
    }

    /// Starts mapping each buffer the renderer asked for since the last frame, or since this was last called, and
    /// answers whether any was asked for.
    pub(crate) fn map(&mut self) -> bool {
        let requests = self.requests.take();
        let asked = !requests.is_empty();
        for mapping in requests {
            let mappings = self.mappings.clone();
            mapping.start(move |mapped| {
                // Refused only once the renderer has gone, and nothing is then owed what it read.
                let _ = mappings.push(mapped);
            });
        }

        asked
    }

    /// Asks for the next frame of each window that shows, its redraw due or not: winit holds a `fifo` window's until
    /// the compositor asks for it, and the redraws that come together are drawn as one frame, as the loop next settles.
    pub(crate) fn ask_next(&self) {
        for opened in self.windows.iter().filter(|opened| opened.shows()) {
            opened.native.request_redraw();
        }
    }

    /// Waits for the GPU to finish what has been submitted, which calls back for each buffer it maps: a held frame
    /// has no next frame to poll the device for it, so what it read back would wait until something else woke the
    /// loop (ADR-0045).
    pub(crate) fn finish(&self) {
        // A device lost is the renderer's to hear of, as it next draws.
        let _ = self.gpu.device().poll(wgpu::PollType::wait_indefinitely());
    }

    /// Polls the device without waiting, between steps: it calls back for each buffer the GPU has mapped by then, and
    /// what became of it is pushed onto the renderer's source.
    pub(crate) fn poll(&self) {
        // A device lost is the renderer's to hear of, as it next draws; polling only calls back what is done.
        let _ = self.gpu.device().poll(wgpu::PollType::Poll);
    }

    /// Between steps, inside one of `event_loop`'s callbacks, where a native window is made: opens `window`, as the
    /// window root asked, titled `title`, at `size` in logical pixels, windowed. It makes the native window, its surface on the
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
        self.windows.push(Kept::new(native, feed, surface, Mode::Windowed));
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
        let native = gpu::native(event_loop, title, LogicalSize::new(size.width, size.height), Mode::Windowed)?;
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
        // Another window may be first now (ADR-0053).
        self.rebudget();
    }
}
