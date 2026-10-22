//! Opens the page's GPU on the canvases opened as windows, as the renderer is bootstrapped: each canvas's surface,
//! configured, and the GPU the renderer draws with (ADR-0010, ADR-0019, ADR-0021, ADR-0024, ADR-0029).

use crate::program::ShownCanvas;
use crate::serving::Surfaced;
use aethereal_render::Gpu;
use aethereal_render::surface::{Front, WindowSurface};
use wasm_bindgen::JsError;

/// WebGPU opened on the canvases, for the renderer's bootstrap to make the renderer's config from: the GPU, the front
/// the renderer is handed each canvas's frame through, and each canvas's surface, configured, in the order the
/// canvases were opened.
pub struct Opened {
    pub gpu: Gpu,
    pub front: Front,
    pub surfaces: Vec<Surfaced>,
}

/// Opens WebGPU on `canvases`, each opened as a window, for the renderer's bootstrap to await: the browser answers
/// each of the GPU's asks with a promise, and a turn never waits on one. The device has timestamps when the adapter
/// offers them, and each canvas can be copied from, so the renderer can time and read back its frames (ADR-0023). The
/// adapter is one that draws to the first canvas. The chassis configures each canvas's surface to each size its window
/// is resized to as the next frame begins, which sets the canvas's backing store, and the page's listeners measure the
/// canvas for it (ADR-0024).
///
/// # Errors
///
/// When the browser has no WebGPU, gives no adapter for the canvases, or refuses a device, or a canvas is drawn to in
/// a format no pass draws into.
pub async fn open(canvases: &[ShownCanvas]) -> Result<Opened, JsError> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::BROWSER_WEBGPU,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let mut surfaces = Vec::with_capacity(canvases.len());
    for shown in canvases {
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(shown.canvas.clone()))
            .map_err(|error| JsError::new(&format!("no WebGPU surface on the canvas: {error}")))?;
        surfaces.push(surface);
    }
    let options = wgpu::RequestAdapterOptions { compatible_surface: surfaces.first(), ..Default::default() };
    let adapter = instance
        .request_adapter(&options)
        .await
        .map_err(|error| JsError::new(&format!("no WebGPU adapter: {error}")))?;
    let required_features = adapter.features() & wgpu::Features::TIMESTAMP_QUERY;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor { required_features, ..Default::default() })
        .await
        .map_err(|error| JsError::new(&format!("the adapter refused a device: {error}")))?;

    let gpu = Gpu::new(device, queue);
    // The chassis's end of the front adds each canvas's slot here, and is dropped: a page opens no window after.
    let (front, fronting) = Front::new();
    let mut kept = Vec::with_capacity(canvases.len());
    for (surface, shown) in surfaces.into_iter().zip(canvases) {
        let (width, height) = (shown.canvas.width().max(1), shown.canvas.height().max(1));
        let mut config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| JsError::new("the adapter cannot draw to the canvas"))?;
        // WebGPU lets a canvas's textures be copied from, as from any texture of its format; wgpu's capabilities for a
        // canvas name only drawing to it, as they are fixed rather than asked of the browser.
        config.usage |= wgpu::TextureUsages::COPY_SRC;
        let surface = WindowSurface::new(surface, &gpu, shown.shown.window(), config, &fronting)
            .map_err(|format| JsError::new(&format!("the canvas draws in {format:?}, which no pass draws into")))?;
        kept.push(Surfaced { surface, size: shown.shown.size() });
    }

    Ok(Opened { gpu, front, surfaces: kept })
}
