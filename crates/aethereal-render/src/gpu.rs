//! The GPU the chassis opens and hands the renderer: a device, its queue, and the surface it draws to (ADR-0010,
//! ADR-0021).

use aethereal_actor::Resource;

/// A device, its queue, and the surface of the canvas or window it draws to, configured to `format`.
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    format: wgpu::TextureFormat,
}

impl Resource for Gpu {}

impl Gpu {
    /// The GPU of `device` and `queue`, drawing to `surface`, which the chassis has configured to `format`.
    #[must_use]
    pub const fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        surface: wgpu::Surface<'static>,
        format: wgpu::TextureFormat,
    ) -> Self {
        Self { device, queue, surface, format }
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

    /// The surface drawn to.
    #[must_use]
    pub const fn surface(&self) -> &wgpu::Surface<'static> {
        &self.surface
    }

    /// The format the surface is configured to.
    #[must_use]
    pub const fn format(&self) -> wgpu::TextureFormat {
        self.format
    }
}
