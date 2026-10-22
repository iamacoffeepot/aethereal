//! The GPU the chassis opens and hands the renderer: a device and its queue. The windows it draws to, each in the
//! format of its own surface, which the chassis keeps, the render root learns through its config (ADR-0010, ADR-0021,
//! ADR-0024).

use aethereal_actor::Resource;

/// A device and its queue. Each window's surface is the chassis's
/// ([`WindowSurface`](crate::surface::WindowSurface)), which hands the renderer each frame's texture.
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl Resource for Gpu {}

impl Gpu {
    /// The GPU of `device` and `queue`.
    #[must_use]
    pub const fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        Self { device, queue }
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
}
