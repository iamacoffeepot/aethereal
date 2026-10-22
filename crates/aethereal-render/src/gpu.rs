//! The GPU the chassis opens and hands the renderer: a device, its queue, and the surface of the window it draws to
//! (ADR-0010, ADR-0021, ADR-0024).

use crate::shape::ColourFormat;
use aethereal_actor::Resource;
use aethereal_window::WindowId;

/// A device, its queue, and the surface of the window it draws to, configured to `config`, whose format is `format`,
/// and the size its window was last heard to be drawn at, `heard`, which the surface is configured to at the next
/// frame.
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    window: WindowId,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    format: ColourFormat,
    heard: (u32, u32),
}

impl Resource for Gpu {}

impl Gpu {
    /// The GPU of `device` and `queue`, drawing to `surface`, the surface of the window `window`, configured here to
    /// `config`. The renderer configures it again whenever its window is resized, and nothing else configures it, so
    /// the size wgpu gives its textures is the size they are.
    ///
    /// # Errors
    ///
    /// When `config` draws in a format no pass draws into, which it answers with.
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        window: WindowId,
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
    ) -> Result<Self, wgpu::TextureFormat> {
        let format = ColourFormat::of(config.format).ok_or(config.format)?;
        surface.configure(&device, &config);
        let heard = (config.width, config.height);

        Ok(Self { device, queue, window, surface, config, format, heard })
    }

    /// The device.
    #[must_use]
    pub const fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// The device's queue.
    #[must_use]
    pub const fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// The window drawn to.
    #[must_use]
    pub const fn window(&self) -> WindowId {
        self.window
    }

    /// The window's surface.
    #[must_use]
    pub const fn surface(&self) -> &wgpu::Surface<'static> {
        &self.surface
    }

    /// The format the surface is configured to.
    #[must_use]
    pub const fn format(&self) -> ColourFormat {
        self.format
    }

    /// Notes that the window is drawn at `width` by `height` of its pixels, for the next frame to configure the
    /// surface to: what is heard more than once before then is configured once, at the last size heard. Each side is
    /// at least one pixel, as a surface of none cannot be configured, and at most the longest side of a texture the
    /// device makes.
    pub(crate) fn heard(&mut self, width: u32, height: u32) {
        let longest = self.device.limits().max_texture_dimension_2d;
        self.heard = (width.clamp(1, longest), height.clamp(1, longest));
    }

    /// Configures the surface to the size its window was last heard at, when that is not the size it is configured
    /// to: called as a frame begins, before its texture is taken, so no frame is drawn at two sizes.
    pub(crate) fn fit(&mut self) {
        let configured = (self.config.width, self.config.height);
        if self.heard == configured {
            return;
        }
        (self.config.width, self.config.height) = self.heard;
        self.surface.configure(&self.device, &self.config);
    }
}
