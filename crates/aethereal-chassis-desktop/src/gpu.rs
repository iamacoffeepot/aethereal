//! Opens the native windows the program asks for and the GPU on them, before the engine is made, and makes the
//! renderer's config from them: the GPU it is handed, and each window's surface, which the chassis keeps (ADR-0001,
//! ADR-0010, ADR-0019, ADR-0021, ADR-0024).

use crate::serving::{Drawing, Opened};
use aethereal_render::surface::{Front, Fronting, WindowSurface};
use aethereal_render::{Gpu, RendererConfig};
use aethereal_substrate::Setup;
use aethereal_window::{Hosting, Opening, Surface, WindowId};
use core::error::Error;
use core::fmt;
use core::mem;
use std::collections::HashMap;
use std::sync::Arc;
use winit::dpi::LogicalSize;
use winit::error::OsError;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

/// The screen, as the program is set up inside the event loop's first callback: it opens each native window the
/// program asks for with [`Screen::open_window`], as a window of the window capability, and the GPU on the first.
pub struct Screen<'a> {
    event_loop: &'a ActiveEventLoop,
    instance: wgpu::Instance,
    device: Device,
    front: Front,
    fronting: Fronting,
    windows: Vec<Opened>,
}

/// The GPU the screen draws with, as it stands.
enum Device {
    /// None yet: it is opened on the first window's surface, so it is one that draws to it.
    Unopened,
    /// Opened: the adapter each window's surface is configured for, and the GPU.
    Opened { adapter: wgpu::Adapter, gpu: Gpu },
}

/// Why a window or the GPU on it did not open.
#[derive(Debug)]
pub enum Unopened {
    /// The windowing library made no window.
    Window(OsError),
    /// wgpu made no surface on the window.
    Surface(wgpu::CreateSurfaceError),
    /// No adapter draws to the window's surface.
    Adapter(wgpu::RequestAdapterError),
    /// The adapter refused a device.
    Device(wgpu::RequestDeviceError),
    /// The adapter cannot draw to the window's surface at all.
    Unsupported,
    /// The surface is drawn to in this format, which no pass draws into.
    Format(wgpu::TextureFormat),
    /// The program opened no window, so no GPU was opened for the renderer.
    NoWindow,
}

impl fmt::Display for Unopened {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Window(error) => write!(f, "no window opened: {error}"),
            Self::Surface(error) => write!(f, "no surface on the window: {error}"),
            Self::Adapter(error) => write!(f, "no adapter draws to the window: {error}"),
            Self::Device(error) => write!(f, "the adapter refused a device: {error}"),
            Self::Unsupported => write!(f, "the adapter cannot draw to the window's surface"),
            Self::Format(format) => write!(f, "the window's surface draws in {format:?}, which no pass draws into"),
            Self::NoWindow => write!(f, "no window was opened to draw to"),
        }
    }
}

impl Error for Unopened {}

impl<'a> Screen<'a> {
    /// The screen of `event_loop`, inside one of its callbacks, the only place a native window is made, with no window
    /// open yet.
    pub(crate) fn new(event_loop: &'a ActiveEventLoop) -> Self {
        let display = Box::new(event_loop.owned_display_handle());
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_with_display_handle_from_env(display)
        });

        let (front, fronting) = Front::new();

        Self { event_loop, instance, device: Device::Unopened, front, fronting, windows: Vec::new() }
    }

    /// Opens a native window titled `title`, its inner size `width` by `height` logical pixels, the screen's scale
    /// giving its surface's pixels, and opens it as the next window of the window capability with `opening`, standing
    /// as it does now: its inner size in its pixels as its surface, the screen's pixels to each logical one as its
    /// scale, and focused when it has the keyboard. Answers the capability's id for it.
    ///
    /// The first window opened opens the GPU on it: an adapter that draws to its surface and a device, each waited for
    /// on this thread, which is the host's to do (ADR-0001 rule 4). The device has timestamps when the adapter offers
    /// them, and each surface can be copied from where the adapter allows it, so the renderer can time and read back
    /// its frames (ADR-0023). Each surface is presented `Fifo`, as the display shows each frame, so acquiring one waits
    /// for the display to free an image, which paces the frames.
    ///
    /// # Errors
    ///
    /// When the window, its surface, or the GPU on it does not open, the adapter cannot draw to it, or its surface is
    /// drawn to in a format no pass draws into.
    pub fn open_window(
        &mut self,
        opening: &mut Opening,
        title: &str,
        (width, height): (u32, u32),
    ) -> Result<WindowId, Unopened> {
        let window = native(self.event_loop, title, LogicalSize::new(f64::from(width), f64::from(height)))?;
        let surface = self.instance.create_surface(Arc::clone(&window)).map_err(Unopened::Surface)?;
        let (adapter, gpu) = match mem::replace(&mut self.device, Device::Unopened) {
            Device::Opened { adapter, gpu } => (adapter, gpu),
            Device::Unopened => opened_on(&self.instance, &surface)?,
        };
        let made = self.surfaced(&adapter, &gpu, opening, window, surface);
        // Kept whether the window's surface is made or not, so a window opened after draws with the same GPU.
        self.device = Device::Opened { adapter, gpu };

        made
    }

    /// Makes the surface of `window`, `surface`, drawn by `gpu` through `adapter`, and opens it as the next window of
    /// the window capability with `opening`.
    fn surfaced(
        &mut self,
        adapter: &wgpu::Adapter,
        gpu: &Gpu,
        opening: &mut Opening,
        window: Arc<Window>,
        surface: wgpu::Surface<'static>,
    ) -> Result<WindowId, Unopened> {
        let config = configured(adapter, &surface, &window)?;
        let feed = opening.open(standing(&window), window.has_focus());
        let surface =
            WindowSurface::new(surface, gpu, feed.window(), config, &self.fronting).map_err(Unopened::Format)?;
        let id = feed.window();
        self.windows.push(Opened::new(window, feed, surface));

        Ok(id)
    }
}

/// A native window titled `title`, its inner size `size` in logical pixels, made on `event_loop`.
pub fn native(event_loop: &ActiveEventLoop, title: &str, size: LogicalSize<f64>) -> Result<Arc<Window>, Unopened> {
    let attributes = Window::default_attributes().with_title(title).with_inner_size(size);

    Ok(Arc::new(event_loop.create_window(attributes).map_err(Unopened::Window)?))
}

/// How the surface of `window`, `surface`, is configured for `adapter`: at the window's inner size, in the surface's
/// own format, presented `Fifo`, and copied from where the adapter allows it.
pub fn configured(
    adapter: &wgpu::Adapter,
    surface: &wgpu::Surface<'_>,
    window: &Window,
) -> Result<wgpu::SurfaceConfiguration, Unopened> {
    let size = window.inner_size();
    let mut config =
        surface.get_default_config(adapter, size.width.max(1), size.height.max(1)).ok_or(Unopened::Unsupported)?;
    config.present_mode = wgpu::PresentMode::Fifo;
    let copied = surface.get_capabilities(adapter).usages & wgpu::TextureUsages::COPY_SRC;
    config.usage |= copied;

    Ok(config)
}

/// The surface of `window` as it stands: its inner size in its pixels, and the screen's pixels to each logical one.
pub fn standing(window: &Window) -> Surface {
    let size = window.inner_size();

    Surface { width: size.width, height: size.height, scale: window.scale_factor() }
}

/// The adapter that draws to `surface`, on `instance`, and the GPU of a device opened on it, each waited for on this
/// thread.
fn opened_on(instance: &wgpu::Instance, surface: &wgpu::Surface<'_>) -> Result<(wgpu::Adapter, Gpu), Unopened> {
    let options = wgpu::RequestAdapterOptions { compatible_surface: Some(surface), ..Default::default() };
    let adapter = pollster::block_on(instance.request_adapter(&options)).map_err(Unopened::Adapter)?;
    let required_features = adapter.features() & wgpu::Features::TIMESTAMP_QUERY;
    let described = wgpu::DeviceDescriptor { required_features, ..Default::default() };
    let (device, queue) = pollster::block_on(adapter.request_device(&described)).map_err(Unopened::Device)?;

    Ok((adapter, Gpu::new(device, queue)))
}

/// The renderer's config, drawing with the GPU opened on the windows of `screen`, made in `setup`, and what the chassis
/// serves it each frame: each window's surface, which hands it the window's frame and presents what it drew, the
/// buffers it asks to have mapped, and each window whose events the chassis pushes. The chassis opens and closes the
/// windows the window root asks for while the program runs through `hosting`, which the root's config came with
/// ([`Opening::config`]), on the same GPU (ADR-0027).
///
/// # Errors
///
/// When the program opened no window on `screen`, so no GPU was opened.
pub fn renderer<S>(
    setup: &mut Setup<S>,
    screen: Screen<'_>,
    hosting: Hosting,
) -> Result<(RendererConfig, Drawing), Unopened> {
    let Screen { instance, device, front, fronting, windows, .. } = screen;
    let Device::Opened { adapter, gpu } = device else {
        return Err(Unopened::NoWindow);
    };
    let kept = Gpu::new(gpu.device().clone(), gpu.queue().clone());
    let formats = windows.iter().map(|opened| opened.surface.drawn()).collect();
    let routes: HashMap<_, _> = windows.iter().map(|opened| (opened.native.id(), opened.feed.window())).collect();
    let (presenting, presents) = setup.controller();
    let (gpu, front) = (setup.hand(gpu), setup.hand(front));
    let (mapping, requests) = setup.controller();
    let (mapped, mappings) = setup.source();
    let (surfaced, surfacing) = setup.source();
    let config = RendererConfig { gpu, front, presenting, windows: formats, mapping, mapped, surfaced };
    let drawing = Drawing {
        windows,
        routes,
        presents,
        requests,
        mappings,
        instance,
        adapter,
        gpu: kept,
        fronting,
        surfacing,
        hosting,
    };

    Ok((config, drawing))
}
