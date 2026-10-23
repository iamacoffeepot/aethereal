//! The renderer bootstrapped on the desktop: the GPU opened on the first window the window root opened, each window's
//! surface configured for it, and what the desktop serves the renderer each frame (ADR-0019, ADR-0021, ADR-0029).

use crate::Presenting;
use crate::gpu::{self, OnScreen, Unopened};
use crate::running::Desktop;
use crate::serving::{Drawing, Kept};
use aethereal_data::Plain;
use aethereal_render::surface::{Front, WindowSurface};
use aethereal_render::{Gpu, Renderer, RendererConfig};
use aethereal_substrate::{Bootstrap, Bootstrapping, Unbuilt};
use aethereal_window::InitialWindows;
use std::collections::HashMap;
use std::sync::Arc;

/// The renderer's settings on the desktop: its section, `renderer` (ADR-0028).
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[kind(name = "aether.desktop.renderer")]
#[settings]
pub struct RendererSettings {
    /// How each window's frames are presented: `fifo`, in step with the display; `uncapped`, as fast as they are
    /// made; or `capped:60`, at most sixty a second.
    #[flag]
    pub present: Presenting,
}

/// Reaches the windows the window root opened through the screen, which keeps them, and makes each one's surface on
/// the screen's instance. Opens the GPU on the first: an adapter that draws to its surface and a device, each waited
/// for on the event loop's thread, which is the host's to do (ADR-0001 rule 4). Then configures each surface for it,
/// presented as the settings say, refused when the surface lacks the mode they ask for, and copied from where the
/// adapter allows it, so the renderer can read back its frames (ADR-0023), and begins the renderer with the GPU, each
/// window's format, and its handles. Gives the desktop its end: each window, sharing the screen's, with its surface,
/// which hands the renderer the window's frame and presents what it drew, the buffers it asks to have mapped, and what
/// the desktop opens the windows the window root asks for with while the program runs, on the same GPU and presented
/// the same (ADR-0027).
impl Bootstrap<Desktop> for Renderer {
    const SECTION: &'static str = "renderer";
    type Settings = RendererSettings;
    type Needs = (InitialWindows,);
    type Provides = ();

    async fn bootstrap(
        settings: RendererSettings,
        _: (InitialWindows,),
        cx: &mut Bootstrapping<'_, Desktop, Self>,
    ) -> Result<(), Unbuilt> {
        let present = settings.present;
        let instance = cx.host().instance().clone();
        let mut surfaced = Vec::new();
        for window in cx.host().windows() {
            let surface = instance.create_surface(Arc::clone(&window.native)).map_err(Unopened::Surface)?;
            surfaced.push((window.clone(), surface));
        }
        let (_, first) = surfaced.first().ok_or(Unopened::NoWindow)?;
        let (adapter, gpu) = gpu::opened_on(&instance, first)?;
        let (front, fronting) = Front::new();
        let mut windows = Vec::with_capacity(surfaced.len());
        for (OnScreen { native, feed, mode }, surface) in surfaced {
            let config = gpu::configured(&adapter, &surface, &native, present)?;
            let surface =
                WindowSurface::new(surface, &gpu, feed.window(), config, &fronting).map_err(Unopened::Format)?;
            windows.push(Kept::new(native, feed, surface, mode));
        }

        let polled = Gpu::new(gpu.device().clone(), gpu.queue().clone());
        let formats = windows.iter().map(|kept| kept.surface.drawn()).collect();
        let routes: HashMap<_, _> = windows.iter().map(|kept| (kept.native.id(), kept.feed.window())).collect();
        let (presenting, presents) = cx.controller();
        let (gpu, front) = (cx.hand(gpu), cx.hand(front));
        let (mapping, requests) = cx.controller();
        let (mapped, mappings) = cx.source();
        let (surfaced, surfacing) = cx.source();
        cx.begin(&(), RendererConfig { gpu, front, presenting, windows: formats, mapping, mapped, surfaced })?;

        cx.serve(Drawing {
            windows,
            routes,
            presents,
            requests,
            mappings,
            instance,
            adapter,
            gpu: polled,
            present,
            fronting,
            surfacing,
        });

        Ok(())
    }
}
