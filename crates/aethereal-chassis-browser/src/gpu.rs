//! The page's GPU: the device, its queue, and the canvas it draws to, opened before the engine is made and handed in
//! as a resource (ADR-0010).

use aethereal_actor::Resource;
use wasm_bindgen::JsError;
use web_sys::HtmlCanvasElement;

/// A WebGPU device and queue, and the surface of the canvas they draw to.
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
}

impl Resource for Gpu {}

impl Gpu {
    /// Opens WebGPU on `canvas`, its surface configured to the canvas's size, for the page to await before it makes
    /// the engine: the browser answers each of these asks with a promise, and a turn never waits on one.
    ///
    /// # Errors
    ///
    /// When the browser has no WebGPU, gives no adapter for the canvas, or refuses a device.
    pub async fn open(canvas: HtmlCanvasElement) -> Result<Self, JsError> {
        let (width, height) = (canvas.width().max(1), canvas.height().max(1));
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|error| JsError::new(&format!("no WebGPU surface on the canvas: {error}")))?;
        let options = wgpu::RequestAdapterOptions { compatible_surface: Some(&surface), ..Default::default() };
        let adapter = instance
            .request_adapter(&options)
            .await
            .map_err(|error| JsError::new(&format!("no WebGPU adapter: {error}")))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|error| JsError::new(&format!("the adapter refused a device: {error}")))?;

        let config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| JsError::new("the adapter cannot draw to the canvas"))?;
        surface.configure(&device, &config);

        Ok(Self { device, queue, surface })
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

    /// The canvas's surface, configured to the canvas's size when it was opened.
    #[must_use]
    pub const fn surface(&self) -> &wgpu::Surface<'static> {
        &self.surface
    }
}
