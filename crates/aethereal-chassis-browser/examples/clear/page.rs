//! The page's entry points, and the root that draws.

use aethereal_actor::{
    Actor, Beginning, ChildOf, Ending, Fault, Given, Handle, Handles, Render, Subscribes, Tick, Top, Turn,
};
use aethereal_chassis_browser::{Browser, Gpu, Performance};
use aethereal_data::{Bound, Kind, KindName};
use aethereal_substrate::Engine;
use core::f64::consts::TAU;
use core::num::NonZeroUsize;
use wasm_bindgen::JsError;
use wasm_bindgen::prelude::wasm_bindgen;
use web_sys::HtmlCanvasElement;

/// The program the page runs, for its `requestAnimationFrame` to call.
#[wasm_bindgen]
pub struct Program {
    browser: Browser<Performance>,
}

/// Opens WebGPU on `canvas`, then makes the engine with the painter as its root, handed the GPU.
///
/// # Errors
///
/// When WebGPU will not open on the canvas, or the painter does not begin.
#[wasm_bindgen]
pub async fn start(canvas: HtmlCanvasElement) -> Result<Program, JsError> {
    let gpu = Gpu::open(canvas).await?;
    let mut setup = Engine::new(NonZeroUsize::MIN, 1024, 1 << 20, 1 << 20, 0)
        .with_cycle::<(Tick, Render)>()
        .map_err(|unordered| JsError::new(&format!("the cycle is out of order: {unordered:?}")))?;
    let gpu = setup.hand(gpu);
    let engine = setup
        .add_root::<Painter>(&(), PainterConfig { gpu })
        .map_err(|refused| JsError::new(&format!("the painter did not begin: {refused:?}")))?
        .ready();

    Ok(Program { browser: Browser::new(engine, Performance) })
}

#[wasm_bindgen]
impl Program {
    /// Runs one frame.
    pub fn frame(&mut self) {
        self.browser.frame();
    }
}

/// The GPU the painter draws with.
#[derive(Bound)]
struct PainterConfig {
    gpu: Given<Gpu>,
}

impl Kind for PainterConfig {
    const NAME: KindName = KindName::new("aether.example.painter_config");
}

/// An actor that clears the canvas on each `Render` to a colour that goes once round the hues every ten seconds of the
/// program's time, as the last `Tick` read it.
#[derive(Bound)]
struct Painter {
    gpu: Handle<Gpu>,
    elapsed_nanos: u64,
}

impl Kind for Painter {
    const NAME: KindName = KindName::new("aether.example.painter");
}

impl ChildOf for Painter {
    type Parent = Top;
}

impl Actor for Painter {
    type Config = PainterConfig;
    type Key = ();
    type Handles = (Tick, Render);
    type BeginSends = (Subscribes<Tick>, Subscribes<Render>);
    type EndSends = ();

    fn begin(config: PainterConfig, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        let gpu = turn.take(config.gpu)?;
        turn.subscribe::<Tick, _>();
        turn.subscribe::<Render, _>();

        Ok(Self { gpu, elapsed_nanos: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Tick> for Painter {
    type Sends = ();

    fn handle(&mut self, tick: Tick, _: &mut Turn<'_, ()>) -> Result<(), Fault> {
        self.elapsed_nanos = tick.elapsed_nanos;

        Ok(())
    }
}

impl Handles<Render> for Painter {
    type Sends = ();

    fn handle(&mut self, _: Render, turn: &mut Turn<'_, ()>) -> Result<(), Fault> {
        let colour = colour(self.elapsed_nanos);
        let gpu = turn.open(&mut self.gpu)?;
        // A frame the canvas has no texture for is skipped; the next asks again.
        let wgpu::CurrentSurfaceTexture::Success(frame) = gpu.surface().get_current_texture() else {
            return Ok(());
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu.device().create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let attachment = wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(colour), store: wgpu::StoreOp::Store },
        };
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(attachment)],
            ..Default::default()
        }));
        gpu.queue().submit([encoder.finish()]);
        gpu.queue().present(frame);

        Ok(())
    }
}

/// The colour `elapsed_nanos` into the program: its hue goes once round every ten seconds.
fn colour(elapsed_nanos: u64) -> wgpu::Color {
    #[allow(clippy::cast_precision_loss)] // A hue needs no more than a double's precision of the time.
    let turn = (elapsed_nanos % 10_000_000_000) as f64 / 10_000_000_000.0 * TAU;
    let channel = |offset: f64| (turn + offset).sin().mul_add(0.5, 0.5);

    wgpu::Color { r: channel(0.0), g: channel(TAU / 3.0), b: channel(2.0 * TAU / 3.0), a: 1.0 }
}
