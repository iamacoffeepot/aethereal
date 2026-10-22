//! Opens the page's GPU on its canvas, the one window, as the engine is set up, and makes the renderer's config from
//! it: the GPU it is handed, and the canvas's surface, which the chassis keeps (ADR-0010, ADR-0019, ADR-0021,
//! ADR-0024).

use crate::canvas::{self, Shown};
use crate::{Drawing, Serving};
use aethereal_render::surface::WindowSurface;
use aethereal_render::{Gpu, RendererConfig};
use aethereal_substrate::Setup;
use aethereal_window::Opening;
use core::cell::Cell;
use std::rc::Rc;
use wasm_bindgen::JsError;
use web_sys::HtmlCanvasElement;

/// WebGPU opened on the canvas, for [`renderer`] to make the renderer's config from: the GPU, and the canvas's
/// surface, to be configured to `config` and then to the window's size as last pushed, `size`.
pub struct Opened {
    gpu: Gpu,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    size: Rc<Cell<(u32, u32)>>,
}

/// Opens WebGPU on `canvas`, then the canvas as a window with `opening`, for the page to await before it makes the
/// engine's roots: the browser answers each of the GPU's asks with a promise, and a turn never waits on one. Answers
/// WebGPU opened, for [`renderer`], and the window, standing as the page has it once the GPU is open. The device has timestamps when the adapter offers them, and the canvas can be
/// copied from, so the renderer can time and read back its frames (ADR-0023). The chassis configures the surface to
/// each size the window is resized to as the next frame begins, which sets the canvas's backing store, and
/// [`canvas::listen`] measures the canvas for it (ADR-0024).
///
/// # Errors
///
/// When the browser has no WebGPU, gives no adapter for the canvas, or refuses a device, the page will not open the
/// canvas as a window, or the canvas is drawn to in a format no pass draws into.
pub async fn open(canvas: HtmlCanvasElement, opening: &mut Opening) -> Result<(Opened, Shown), JsError> {
    let (width, height) = (canvas.width().max(1), canvas.height().max(1));
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::BROWSER_WEBGPU,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
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
    // Opened once nothing is awaited: what it shares with the page's listeners stays on the page's thread.
    let shown = canvas::open(&canvas, opening)?;

    let mut config = surface
        .get_default_config(&adapter, width, height)
        .ok_or_else(|| JsError::new("the adapter cannot draw to the canvas"))?;
    // WebGPU lets a canvas's textures be copied from, as from any texture of its format; wgpu's capabilities for a
    // canvas name only drawing to it, as they are fixed rather than asked of the browser.
    config.usage |= wgpu::TextureUsages::COPY_SRC;
    let gpu = Gpu::new(device, queue, shown.window(), config.format)
        .map_err(|format| JsError::new(&format!("the canvas draws in {format:?}, which no pass draws into")))?;
    let opened = Opened { gpu, surface, config, size: shown.size() };

    Ok((opened, shown))
}

/// The renderer's config, drawing with the GPU `opened`, made in `setup`, and what the chassis serves it each frame:
/// the canvas's surface, configured here, which hands it each frame and presents what it drew, and the buffers it asks
/// to have mapped.
pub fn renderer<S>(setup: &mut Setup<S>, opened: Opened) -> (RendererConfig, Serving) {
    let Opened { gpu, surface, config, size } = opened;
    let (presenting, presents) = setup.controller();
    let (surface, front) = WindowSurface::new(surface, &gpu, config, presents);
    let (gpu, front) = (setup.hand(gpu), setup.hand(front));
    let (mapping, requests) = setup.controller();
    let (mapped, feed) = setup.source();
    let config = RendererConfig { gpu, front, presenting, mapping, mapped };

    (config, Serving::Drawing(Box::new(Drawing { surface, size, requests, feed })))
}
