//! Opens the native window and the GPU on it, before the engine is made, and makes the renderer's config from them:
//! the GPU it is handed, and the window's surface, which the chassis keeps (ADR-0001, ADR-0010, ADR-0019, ADR-0021,
//! ADR-0024).

use crate::serving::Drawing;
use aethereal_render::surface::WindowSurface;
use aethereal_render::{Gpu, RendererConfig};
use aethereal_substrate::Setup;
use aethereal_window::{Opening, Surface, WindowFeed, WindowId};
use core::error::Error;
use core::fmt;
use std::sync::Arc;
use winit::error::OsError;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes};

/// The native window, open, and the GPU opened on it, for the program to open as a window of the window capability
/// with [`Screen::open`] as it sets the engine up.
pub struct Screen {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
}

/// The screen opened as a window of the window capability, for [`renderer`] to make the renderer's config from: the
/// GPU, the window's surface, to be configured to `config`, and the feed of what the window hears.
pub struct Opened {
    gpu: Gpu,
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    feed: WindowFeed,
}

/// Why the window or the GPU on it did not open.
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
        }
    }
}

impl Error for Unopened {}

impl Screen {
    /// Opens a window as `attributes` say on `event_loop`, inside one of its callbacks, the only place a native window
    /// is made, and the GPU on it: the instance, the window's surface, an adapter that draws to it, and a device, each
    /// waited for on this thread, which is the host's to do (ADR-0001 rule 4). The device has timestamps when the
    /// adapter offers them, and the surface can be copied from where the adapter allows it, so the renderer can time and
    /// read back its frames (ADR-0023). The surface is presented `Fifo`, as the display shows each frame, so acquiring
    /// one waits for the display to free an image, which paces the frames.
    pub(crate) fn open(event_loop: &ActiveEventLoop, attributes: WindowAttributes) -> Result<Self, Unopened> {
        let window = Arc::new(event_loop.create_window(attributes).map_err(Unopened::Window)?);
        let display = Box::new(event_loop.owned_display_handle());
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_with_display_handle_from_env(display)
        });
        let surface = instance.create_surface(Arc::clone(&window)).map_err(Unopened::Surface)?;
        let options = wgpu::RequestAdapterOptions { compatible_surface: Some(&surface), ..Default::default() };
        let adapter = pollster::block_on(instance.request_adapter(&options)).map_err(Unopened::Adapter)?;
        let required_features = adapter.features() & wgpu::Features::TIMESTAMP_QUERY;
        let described = wgpu::DeviceDescriptor { required_features, ..Default::default() };
        let (device, queue) = pollster::block_on(adapter.request_device(&described)).map_err(Unopened::Device)?;

        let size = window.inner_size();
        let mut config =
            surface.get_default_config(&adapter, size.width.max(1), size.height.max(1)).ok_or(Unopened::Unsupported)?;
        config.present_mode = wgpu::PresentMode::Fifo;
        let copied = surface.get_capabilities(&adapter).usages & wgpu::TextureUsages::COPY_SRC;
        config.usage |= copied;

        Ok(Self { window, surface, device, queue, config })
    }

    /// Opens the screen as a window of the window capability with `opening`, standing as it does now: its inner size
    /// in its pixels as its surface, the screen's pixels to each logical one as its scale, and focused when it has the
    /// keyboard.
    ///
    /// # Errors
    ///
    /// When the window's surface is drawn to in a format no pass draws into.
    pub fn open_window(self, opening: &mut Opening) -> Result<Opened, Unopened> {
        let Self { window, surface, device, queue, config } = self;
        let size = window.inner_size();
        let standing = Surface { width: size.width, height: size.height, scale: window.scale_factor() };
        let feed = opening.open(standing, window.has_focus());
        let gpu = Gpu::new(device, queue, feed.window(), config.format).map_err(Unopened::Format)?;

        Ok(Opened { gpu, window, surface, config, feed })
    }
}

impl Opened {
    /// The window, by the window capability's id for it.
    #[must_use]
    pub const fn window(&self) -> WindowId {
        self.feed.window()
    }
}

/// The renderer's config, drawing with the GPU `opened`, made in `setup`, and what the chassis serves it each frame:
/// the window's surface, configured here, which hands it each frame and presents what it drew, the buffers it asks to
/// have mapped, and the window whose events the chassis pushes.
pub fn renderer<S>(setup: &mut Setup<S>, opened: Opened) -> (RendererConfig, Drawing) {
    let Opened { gpu, window, surface, config, feed } = opened;
    let device = gpu.device().clone();
    let (presenting, presents) = setup.controller();
    let (surface, front) = WindowSurface::new(surface, &gpu, config, presents);
    let (gpu, front) = (setup.hand(gpu), setup.hand(front));
    let (mapping, requests) = setup.controller();
    let (mapped, mappings) = setup.source();
    let config = RendererConfig { gpu, front, presenting, mapping, mapped };

    (config, Drawing { window, feed, surface, device, requests, mappings })
}
