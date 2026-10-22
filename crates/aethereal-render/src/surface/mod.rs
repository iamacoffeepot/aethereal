//! The surface of the window the render root draws to, which its chassis keeps, and the front it hands the render root
//! each frame through (ADR-0019, ADR-0021, ADR-0024).
//!
//! A frame of a surface is bound to its host: it is presented by value, and natively it is acquired by waiting for the
//! display to free an image. So the chassis keeps the surface and each frame it acquires, and the render root holds
//! a front from boot, a slot the chassis puts the frame's texture into, and a controller it asks for the frame to be
//! presented through. Each cycle the chassis acquires on its own thread, [`WindowSurface::acquire`], runs the cycle,
//! whose `Render` takes the texture, draws into it, and pushes [`Present`], and then presents,
//! [`WindowSurface::present`]. No turn waits on the display, and nothing is made each frame.

mod front;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;

pub use front::{Front, Present};

use crate::Gpu;
use aethereal_actor::Requests;
use front::Handing;
use std::mem;

/// A window's surface, as its chassis keeps it: configured in the format of the GPU that draws to it, at the size its
/// window was last heard to be drawn at, `heard`, as the next frame is acquired. It keeps the frame it acquired, and
/// hands the render root its texture through the front.
pub struct WindowSurface {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    heard: (u32, u32),
    configured: Configured,
    front: Handing,
    presents: Requests<Present>,
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
    /// `surface`, drawn to by `gpu`, configured here to `config` in the GPU's format, with the front the render root
    /// is handed each frame through, and `presents`, the chassis's end of the controller the render root asks for each
    /// frame to be presented through. The chassis configures it again whenever its window is resized, and nothing else
    /// configures it, so the size wgpu gives its textures is the size they are.
    #[must_use]
    pub fn new(
        surface: wgpu::Surface<'static>,
        gpu: &Gpu,
        config: wgpu::SurfaceConfiguration,
        presents: Requests<Present>,
    ) -> (Self, Front) {
        let config = wgpu::SurfaceConfiguration { format: gpu.format().wgpu(), ..config };
        surface.configure(gpu.device(), &config);
        let heard = (config.width, config.height);
        let (front, handing) = Front::new();
        let opened = Self {
            surface,
            device: gpu.device().clone(),
            queue: gpu.queue().clone(),
            config,
            heard,
            configured: Configured::Fits,
            front: handing,
            presents,
            kept: Kept::Nothing,
        };

        (opened, front)
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

    /// Takes what the render root asked of the cycle's frame, after the cycle, and presents the frame kept when it
    /// asked for it to be presented: a frame it did not draw, or drew in a turn that failed before it asked, is
    /// dropped, and a texture it did not take is taken back from its front.
    pub fn present(&mut self) {
        let asked = !self.presents.take().is_empty();
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
