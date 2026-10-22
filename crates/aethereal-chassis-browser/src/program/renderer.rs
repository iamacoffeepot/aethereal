//! The renderer bootstrapped on the browser: WebGPU opened on the canvases the window root opened, awaited, and what
//! the browser serves the renderer each frame (ADR-0019, ADR-0021, ADR-0029).

use super::unopened;
use crate::gpu::{self, Opened};
use crate::serving::Drawing;
use crate::{Browser, Performance};
use aethereal_render::{Renderer, RendererConfig};
use aethereal_substrate::{Bootstrap, Bootstrapping, Unbuilt};
use aethereal_window::InitialWindows;

/// Opens WebGPU on the canvases the window root opened, reached through the page, which keeps them, awaiting the
/// browser's answers, then begins the renderer with the GPU, each canvas's format, and its handles. Gives the browser
/// its end: each canvas's surface, made on the canvas the page keeps, which hands the renderer the canvas's frame and
/// presents what it drew, and the buffers it asks to have mapped. The source the
/// renderer hears of windows opened and closed through is fed nothing, as a page opens and closes no window while the
/// program runs (ADR-0027).
impl Bootstrap<Browser<Performance>> for Renderer {
    const SECTION: &'static str = "renderer";
    type Settings = ();
    type Needs = (InitialWindows,);
    type Provides = ();

    async fn bootstrap(
        (): (),
        _: (InitialWindows,),
        cx: &mut Bootstrapping<'_, Browser<Performance>, Self>,
    ) -> Result<(), Unbuilt> {
        let Opened { gpu, front, surfaces } = gpu::open(cx.host().canvases()).await.map_err(unopened)?;

        let windows = surfaces.iter().map(|surfaced| surfaced.surface.drawn()).collect();
        let (presenting, presents) = cx.controller();
        let (gpu, front) = (cx.hand(gpu), cx.hand(front));
        let (mapping, requests) = cx.controller();
        let (mapped, feed) = cx.source();
        let (surfaced, _) = cx.source();
        cx.begin(&(), RendererConfig { gpu, front, presenting, windows, mapping, mapped, surfaced })?;
        cx.serve(Drawing { surfaces, presents, requests, feed });

        Ok(())
    }
}
