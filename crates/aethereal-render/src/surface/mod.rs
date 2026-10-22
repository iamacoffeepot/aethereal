//! The surface of each window the render root draws to, which its chassis keeps, and the one front it hands the render
//! root each window's frame through (ADR-0019, ADR-0021, ADR-0024).
//!
//! A frame of a surface is bound to its host: it is presented by value, and natively it is acquired by waiting for the
//! display to free an image. So the chassis keeps each window's surface and each frame it acquires, and the render root
//! holds a front from boot, a slot for each window the chassis puts the window's frame's texture into, and a controller
//! it asks for each window's frame to be presented through. Each cycle the chassis acquires each shown window's frame
//! in turn on its own thread, [`WindowSurface::acquire`], runs the cycle, whose `Render` takes the textures, draws into
//! each, and pushes [`Present`] for each, and then presents each, [`WindowSurface::present`]. No turn waits on the
//! display, and nothing is made each frame.

mod front;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;

pub use front::{Front, Present};

use crate::Gpu;
use crate::shape::ColourFormat;
use aethereal_data::Plain;
use aethereal_window::WindowId;
use front::Handing;
use std::mem;

/// A window the render root draws to, and the format of its surface: what its config names of each window, as its
/// chassis makes it from the window's [`WindowSurface`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct WindowFormat {
    pub window: WindowId,
    pub format: ColourFormat,
}

/// A window's surface, as its chassis keeps it: configured in the format of its own, at the size its window was last
/// heard to be drawn at, `heard`, as the next frame is acquired. It keeps the frame it acquired, and hands the render
/// root its texture through the window's slot in the front.
pub struct WindowSurface {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    drawn: WindowFormat,
    config: wgpu::SurfaceConfiguration,
    heard: (u32, u32),
    configured: Configured,
    front: Handing,
    kept: Kept,
}

/// Whether the surface's frames fit it as it is configured.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Configured {
    /// They do.
    Fits,
    /// The last frame acquired no longer matched the surface, which is configured again before the next.
    Stale,
}

/// The frame the chassis keeps for the cycle it is acquired for.
enum Kept {
    /// A frame whose texture was handed to the render root, kept to present.
    Frame(wgpu::SurfaceTexture),
    /// None: none was acquired, or the one kept was presented or dropped.
    Nothing,
}

impl WindowSurface {
    /// `surface`, the surface of `window`, drawn to by `gpu`, configured here to `config` in the format it names, with
    /// the window's slot in `front`, the front the render root is handed each window's frame through. The chassis
    /// configures it again whenever its window is resized, and nothing else configures it, so the size wgpu gives its
    /// textures is the size they are.
    ///
    /// # Errors
    ///
    /// When `config` names a format no pass draws into, which it answers with.
    pub fn new(
        surface: wgpu::Surface<'static>,
        gpu: &Gpu,
        window: WindowId,
        config: wgpu::SurfaceConfiguration,
        front: &mut Front,
    ) -> Result<Self, wgpu::TextureFormat> {
        let format = ColourFormat::of(config.format).ok_or(config.format)?;
        surface.configure(gpu.device(), &config);
        let heard = (config.width, config.height);

        Ok(Self {
            surface,
            device: gpu.device().clone(),
            queue: gpu.queue().clone(),
            drawn: WindowFormat { window, format },
            config,
            heard,
            configured: Configured::Fits,
            front: front.hand(window),
            kept: Kept::Nothing,
        })
    }

    /// The window it is the surface of, and its format, as the render root's config names it.
    #[must_use]
    pub const fn drawn(&self) -> WindowFormat {
        self.drawn
    }

    /// Notes that the window is drawn at `width` by `height` of its pixels, for the next frame to be acquired at: what
    /// is heard more than once before then is configured once, at the last size heard. A side of no pixels acquires no
    /// frame until the window is drawn at a size again, and a side is at most the longest of a texture the device
    /// makes.
    pub fn resized(&mut self, width: u32, height: u32) {
        let longest = self.device.limits().max_texture_dimension_2d;
        self.heard = (width.min(longest), height.min(longest));
    }

    /// Acquires the frame for the next cycle, before it runs, and puts its texture in the render root's front: on the
    /// chassis's thread, where natively it waits until the display frees an image, which is what paces the cycles. A
    /// frame kept and not presented is dropped first. The surface is configured first to the size last heard when it
    /// is not configured to it, or when the last frame no longer matched it.
    ///
    /// Nothing is handed for a window of no size, or when the surface gives no frame: when it timed out, the window
    /// is hidden, or the surface is outdated or lost, which configures it again for the next. A frame that no longer
    /// matches the surface is handed, and the surface configured again before the next.
    pub fn acquire(&mut self) {
        drop(self.release());
        let (width, height) = self.heard;
        let empty = width == 0 || height == 0;
        if empty {
            return;
        }
        let resized = self.heard != (self.config.width, self.config.height);
        let stale = self.configured == Configured::Stale;
        if resized || stale {
            self.configure();
        }

        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => self.hand(frame),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                self.hand(frame);
                self.configured = Configured::Stale;
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => self.configure(),
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => {}
        }
    }

    /// After the cycle, presents the frame kept when the render root asked, among `asked`, what it asked of the
    /// cycle's frames, for this window's to be presented: a frame it did not draw, or drew in a turn that failed
    /// before it asked, is dropped, and a texture it did not take is taken back from its front. What it asked for
    /// another window's frame is that window's.
    pub fn present(&mut self, asked: &[Present]) {
        let asked = Present::asked_for(asked, self.drawn.window);
        let kept = self.release();
        if asked && let Kept::Frame(frame) = kept {
            self.queue.present(frame);
        }
    }

    /// Configures the surface to the size last heard.
    fn configure(&mut self) {
        (self.config.width, self.config.height) = self.heard;
        self.surface.configure(&self.device, &self.config);
        self.configured = Configured::Fits;
    }

    /// Keeps `frame`, and puts its texture in the render root's front.
    fn hand(&mut self, frame: wgpu::SurfaceTexture) {
        self.front.put(frame.texture.clone());
        self.kept = Kept::Frame(frame);
    }

    /// The frame kept, no longer kept, its texture taken back from the front if the render root did not take it.
    fn release(&mut self) -> Kept {
        self.front.clear();

        mem::replace(&mut self.kept, Kept::Nothing)
    }
}
