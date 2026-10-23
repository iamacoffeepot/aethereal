//! The screen a program is bootstrapped on, inside the event loop's first callback: the native windows its window root
//! asks for, which the screen keeps, and the GPU its renderer opens on them (ADR-0001, ADR-0019, ADR-0021, ADR-0024,
//! ADR-0029).

use crate::Presenting;
use crate::moding;
use crate::presenting::Unoffered;
use crate::serving::Drawing;
use aethereal_chassis_headless::Serving;
use aethereal_files::FileHosting;
use aethereal_render::Gpu;
use aethereal_substrate::Unbuilt;
use aethereal_window::{Hosting, Mode, Opening, Surface, WindowFeed, WindowId};
use core::error::Error;
use core::fmt;
use std::sync::Arc;
use winit::dpi::LogicalSize;
use winit::error::OsError;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

/// The screen, as the program is bootstrapped inside the event loop's first callback: the desktop's hosting
/// (ADR-0029). It opens each native window the window root's bootstrap asks for and keeps it, for the renderer's
/// bootstrap to reach through the host and make each window's surface on, and keeps the ends the roots give the host.
pub struct Screen<'a> {
    event_loop: &'a ActiveEventLoop,
    instance: wgpu::Instance,
    windows: Vec<OnScreen>,
    windowing: Served<Hosting>,
    drawing: Served<Box<Drawing>>,
    /// What the roots served for the host's runners: the file capability's storage (ADR-0029, ADR-0032).
    runners: Serving,
}

/// An end the host keeps, as the roots have given it.
enum Served<E> {
    /// A root gave it.
    Served(E),
    /// No root has.
    Unserved,
}

/// A native window opened as the program is set up, the feed of what it hears, and the mode it was opened in: the
/// screen's, which the renderer's bootstrap shares to make the window's surface on the GPU it opens and to draw it.
#[derive(Clone)]
pub struct OnScreen {
    pub(crate) native: Arc<Window>,
    pub(crate) feed: WindowFeed,
    pub(crate) mode: Mode,
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
    /// The surface offers no mode the windows are presented as.
    Presenting(Unoffered),
    /// No window was opened as the program was set up, so no GPU was opened for the renderer.
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
            Self::Presenting(unoffered) => write!(f, "{unoffered}"),
            Self::NoWindow => write!(f, "no window was opened to draw to"),
        }
    }
}

impl Error for Unopened {}

impl From<Unopened> for Unbuilt {
    fn from(unopened: Unopened) -> Self {
        Self::Unopened(Box::new(unopened))
    }
}

impl<'a> Screen<'a> {
    /// The screen of `event_loop`, inside one of its callbacks, the only place a native window is made, with no window
    /// open yet and no end kept.
    pub(crate) fn new(event_loop: &'a ActiveEventLoop) -> Self {
        let display = Box::new(event_loop.owned_display_handle());
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_with_display_handle_from_env(display)
        });

        let windows = Vec::new();

        Self {
            event_loop,
            instance,
            windows,
            windowing: Served::Unserved,
            drawing: Served::Unserved,
            runners: Serving::new(),
        }
    }

    /// Opens a native window titled `title`, its inner size `width` by `height` logical pixels, the screen's scale
    /// giving its surface's pixels, shown in `mode`, keeps it, and opens it as the next window of the window capability
    /// with `opening`, standing as it does now: its inner size in its pixels as its surface, the screen's pixels to
    /// each logical one as its scale, the mode it was opened in, and focused when it has the keyboard. Answers the
    /// capability's id for it.
    ///
    /// # Errors
    ///
    /// When the window does not open.
    pub(crate) fn open_window(
        &mut self,
        opening: &mut Opening,
        title: &str,
        (width, height): (u32, u32),
        mode: Mode,
    ) -> Result<WindowId, Unopened> {
        let native = native(self.event_loop, title, LogicalSize::new(f64::from(width), f64::from(height)), mode)?;
        let feed = opening.open(standing(&native), mode, native.has_focus());
        let window = feed.window();
        self.windows.push(OnScreen { native, feed, mode });

        Ok(window)
    }

    /// The instance the windows' surfaces are made on.
    pub(crate) const fn instance(&self) -> &wgpu::Instance {
        &self.instance
    }

    /// The windows opened as the program is set up, in the order opened.
    pub(crate) fn windows(&self) -> &[OnScreen] {
        &self.windows
    }

    /// Keeps the window root's end, `hosting`, in place of any kept before.
    pub(crate) fn serve_windowing(&mut self, hosting: Hosting) {
        self.windowing = Served::Served(hosting);
    }

    /// Keeps the renderer's end, `drawing`, in place of any kept before.
    pub(crate) fn serve_drawing(&mut self, drawing: Drawing) {
        self.drawing = Served::Served(Box::new(drawing));
    }

    /// Keeps what the file capability's bootstrap served, for the runners the host makes (ADR-0032).
    pub(crate) fn serve_files(&mut self, files: FileHosting) {
        self.runners.files(files);
    }

    /// What the roots served: the renderer's end, the window root's, and what the host's runners are made with.
    ///
    /// # Errors
    ///
    /// The end no root served, by name: the desktop draws each window it opens through the renderer, and opens and
    /// closes them as the window root asks.
    pub(crate) fn into_served(self) -> Result<(Box<Drawing>, Hosting, Serving), &'static str> {
        match (self.drawing, self.windowing) {
            (Served::Served(drawing), Served::Served(windowing)) => Ok((drawing, windowing, self.runners)),
            (Served::Unserved, _) => Err("the renderer's drawing"),
            (Served::Served(_), Served::Unserved) => Err("the window root's hosting"),
        }
    }
}

/// A native window titled `title`, its inner size `size` in logical pixels, shown in `mode`, made on `event_loop`.
pub fn native(
    event_loop: &ActiveEventLoop,
    title: &str,
    size: LogicalSize<f64>,
    mode: Mode,
) -> Result<Arc<Window>, Unopened> {
    let attributes = moding::opened_in(Window::default_attributes().with_title(title).with_inner_size(size), mode);

    Ok(Arc::new(event_loop.create_window(attributes).map_err(Unopened::Window)?))
}

/// How the surface of `window`, `surface`, is configured for `adapter`: at the window's inner size, in the surface's
/// own format, presented with the mode `present` asks for, and copied from where the adapter allows it. A surface that
/// offers no mode `present` asks for is refused, naming those it offers.
pub fn configured(
    adapter: &wgpu::Adapter,
    surface: &wgpu::Surface<'_>,
    window: &Window,
    present: Presenting,
) -> Result<wgpu::SurfaceConfiguration, Unopened> {
    let size = window.inner_size();
    let mut config =
        surface.get_default_config(adapter, size.width.max(1), size.height.max(1)).ok_or(Unopened::Unsupported)?;
    let capabilities = surface.get_capabilities(adapter);
    config.present_mode = present.mode(&capabilities.present_modes).map_err(Unopened::Presenting)?;
    config.usage |= capabilities.usages & wgpu::TextureUsages::COPY_SRC;

    Ok(config)
}

/// The surface of `window` as it stands: its inner size in its pixels, and the screen's pixels to each logical one.
pub fn standing(window: &Window) -> Surface {
    let size = window.inner_size();

    Surface { width: size.width, height: size.height, scale: window.scale_factor() }
}

/// The adapter that draws to `surface`, on `instance`, and the GPU of a device opened on it, each waited for on this
/// thread, which is the host's to do (ADR-0001 rule 4). The device has timestamps when the adapter offers them, so the
/// renderer can time its frames (ADR-0023).
pub fn opened_on(instance: &wgpu::Instance, surface: &wgpu::Surface<'_>) -> Result<(wgpu::Adapter, Gpu), Unopened> {
    let options = wgpu::RequestAdapterOptions { compatible_surface: Some(surface), ..Default::default() };
    let adapter = pollster::block_on(instance.request_adapter(&options)).map_err(Unopened::Adapter)?;
    let required_features = adapter.features() & wgpu::Features::TIMESTAMP_QUERY;
    let described = wgpu::DeviceDescriptor { required_features, ..Default::default() };
    let (device, queue) = pollster::block_on(adapter.request_device(&described)).map_err(Unopened::Device)?;

    Ok((adapter, Gpu::new(device, queue)))
}
