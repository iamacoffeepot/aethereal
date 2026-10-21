//! The GPU the chassis opens and hands the renderer: a device, its queue, and the surface of the window it draws to
//! (ADR-0010, ADR-0021, ADR-0024).

use crate::shape::ColourFormat;
use aethereal_actor::Resource;
use aethereal_window::WindowId;

/// A device, its queue, and the surface of the window it draws to, configured to `format`.
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    window: WindowId,
    surface: wgpu::Surface<'static>,
    format: ColourFormat,
}

impl Resource for Gpu {}

impl Gpu {
    /// The GPU of `device` and `queue`, drawing to `surface`, the surface of the window `window`, which the chassis
    /// has configured to `format`.
    #[must_use]
    pub const fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        window: WindowId,
        surface: wgpu::Surface<'static>,
        format: ColourFormat,
    ) -> Self {
        Self { device, queue, window, surface, format }
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
}
