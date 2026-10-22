//! Opens the page's GPU on its canvases, each a window, as the engine is set up, and makes the renderer's config from
//! it: the GPU it is handed, and each canvas's surface, which the chassis keeps (ADR-0010, ADR-0019, ADR-0021,
//! ADR-0024).

use crate::canvas::{Page, Shown};
use crate::serving::{Drawing, Serving, Surfaced};
use aethereal_render::surface::{Front, WindowSurface};
use aethereal_render::{Gpu, RendererConfig};
use aethereal_substrate::Setup;
use aethereal_window::Opening;
use wasm_bindgen::JsError;
use web_sys::HtmlCanvasElement;

/// WebGPU opened on the canvases, for [`renderer`] to make the renderer's config from: the GPU, the front the renderer
/// is handed each canvas's frame through, and each canvas's surface, configured, in the order the canvases were given.
pub struct Opened {
    gpu: Gpu,
    front: Front,
    surfaces: Vec<Surfaced>,
}

/// Opens WebGPU on `canvases`, then each canvas as a window with `opening`, `0`, `1`, … in the order given, for the
/// page to await before it makes the engine's roots: the browser answers each of the GPU's asks with a promise, and a
/// turn never waits on one. Answers WebGPU opened, for [`renderer`], and each window, standing as the page has it once
/// the GPU is open, in the order given. The device has timestamps when the adapter offers them, and each canvas can be
/// copied from, so the renderer can time and read back its frames (ADR-0023). The adapter is one that draws to the
/// first canvas. The chassis configures each canvas's surface to each size its window is resized to as the next frame
/// begins, which sets the canvas's backing store, and [`canvas::listen`](crate::canvas::listen) measures the canvas for
/// it (ADR-0024).
///
/// # Errors
///
/// When the browser has no WebGPU, gives no adapter for the canvases, or refuses a device, the page will not open a
/// canvas as a window, or a canvas is drawn to in a format no pass draws into.
pub async fn open<const N: usize>(
    canvases: [HtmlCanvasElement; N],
    opening: &mut Opening,
) -> Result<(Opened, [Shown; N]), JsError> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::BROWSER_WEBGPU,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let mut surfaces = Vec::with_capacity(N);
    for canvas in &canvases {
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
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
    // Opened once nothing is awaited: what it shares with the page's listeners stays on the page's thread.
    let page = Page::now()?;
    let shown = canvases.each_ref().map(|canvas| page.open(canvas, opening));

    let gpu = Gpu::new(device, queue);
    let mut front = Front::new();
    let mut kept = Vec::with_capacity(N);
    for ((surface, canvas), shown) in surfaces.into_iter().zip(&canvases).zip(&shown) {
        let (width, height) = (canvas.width().max(1), canvas.height().max(1));
        let mut config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| JsError::new("the adapter cannot draw to the canvas"))?;
        // WebGPU lets a canvas's textures be copied from, as from any texture of its format; wgpu's capabilities for a
        // canvas name only drawing to it, as they are fixed rather than asked of the browser.
        config.usage |= wgpu::TextureUsages::COPY_SRC;
        let surface = WindowSurface::new(surface, &gpu, shown.window(), config, &mut front)
            .map_err(|format| JsError::new(&format!("the canvas draws in {format:?}, which no pass draws into")))?;
        kept.push(Surfaced { surface, size: shown.size() });
    }

    Ok((Opened { gpu, front, surfaces: kept }, shown))
}

/// The renderer's config, drawing with the GPU `opened`, made in `setup`, and what the chassis serves it each frame:
/// each canvas's surface, which hands it the canvas's frame and presents what it drew, and the buffers it asks to have
/// mapped.
pub fn renderer<S>(setup: &mut Setup<S>, opened: Opened) -> (RendererConfig, Serving) {
    let Opened { gpu, front, surfaces } = opened;
    let windows = surfaces.iter().map(|surfaced| surfaced.surface.drawn()).collect();
    let (presenting, presents) = setup.controller();
    let (gpu, front) = (setup.hand(gpu), setup.hand(front));
    let (mapping, requests) = setup.controller();
    let (mapped, feed) = setup.source();
    let config = RendererConfig { gpu, front, presenting, windows, mapping, mapped };

    (config, Serving::Drawing(Box::new(Drawing { surfaces, presents, requests, feed })))
}
