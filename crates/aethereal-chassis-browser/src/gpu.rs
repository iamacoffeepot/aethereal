//! Opens the page's GPU on its canvas, the one window, as the engine is set up, to hand to the renderer (ADR-0010,
//! ADR-0021, ADR-0024).

use crate::canvas;
use aethereal_render::Gpu;
use aethereal_render::shape::ColourFormat;
use aethereal_window::{Opening, WindowFeed};
use wasm_bindgen::JsError;
use web_sys::HtmlCanvasElement;

/// Opens `canvas` as a window with `opening`, then WebGPU on it, its surface configured to the canvas's size, for the
/// page to await before it makes the engine's roots: the browser answers each of these asks with a promise, and a turn
/// never waits on one. Answers the GPU, drawing to the window, and the window's feed. The device has
/// timestamps when the adapter offers them, and the canvas can be copied from, so the renderer can time and read back
/// its frames (ADR-0023).
///
/// # Errors
///
/// When the page will not open the canvas as a window, the browser has no WebGPU, gives no adapter for the canvas,
/// refuses a device, or draws to the canvas in a format no pass draws into.
pub async fn open(canvas: HtmlCanvasElement, opening: &mut Opening) -> Result<(Gpu, WindowFeed), JsError> {
    let shown = canvas::open(&canvas, opening)?;
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
    let required_features = adapter.features() & wgpu::Features::TIMESTAMP_QUERY;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor { required_features, ..Default::default() })
        .await
        .map_err(|error| JsError::new(&format!("the adapter refused a device: {error}")))?;

    let mut config = surface
        .get_default_config(&adapter, width, height)
        .ok_or_else(|| JsError::new("the adapter cannot draw to the canvas"))?;
    // WebGPU lets a canvas's textures be copied from, as from any texture of its format; wgpu's capabilities for a
    // canvas name only drawing to it, as they are fixed rather than asked of the browser.
    config.usage |= wgpu::TextureUsages::COPY_SRC;
    let format = ColourFormat::of(config.format)
        .ok_or_else(|| JsError::new(&format!("the canvas draws in {:?}, which no pass draws into", config.format)))?;
    surface.configure(&device, &config);

    Ok((Gpu::new(device, queue, shown.window(), surface, format), shown))
}
