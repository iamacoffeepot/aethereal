//! The GPU the chassis opens and hands the renderer: a device, its queue, and the window it draws to, in the format of
//! that window's surface, which the chassis keeps (ADR-0010, ADR-0021, ADR-0024).

use crate::shape::ColourFormat;
use aethereal_actor::Resource;
use aethereal_window::WindowId;

/// A device, its queue, and the window it draws to, whose surface is of `format`. The surface is the chassis's
/// ([`WindowSurface`](crate::surface::WindowSurface)), which hands the renderer each frame's texture.
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    window: WindowId,
    format: ColourFormat,
}

impl Resource for Gpu {}

impl Gpu {
    /// The GPU of `device` and `queue`, drawing to the window `window`, whose surface is of `format`.
    ///
    /// # Errors
    ///
    /// When `format` is one no pass draws into, which it answers with.
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        window: WindowId,
        format: wgpu::TextureFormat,
    ) -> Result<Self, wgpu::TextureFormat> {
        let format = ColourFormat::of(format).ok_or(format)?;

        Ok(Self { device, queue, window, format })
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

    /// The format of the window's surface.
    #[must_use]
    pub const fn format(&self) -> ColourFormat {
        self.format
    }
}
