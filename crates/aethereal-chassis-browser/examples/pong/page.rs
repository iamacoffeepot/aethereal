//! The page's entry points: the engine made with the window root, the renderer, and the game as its roots, and the
//! page's keys listened to for the game.

use crate::game::{Match, Pong};
use aethereal_actor::{Render, Tick};
use aethereal_chassis_browser::{Browser, Listening, Performance, canvas, open, renderer};
use aethereal_render::Renderer;
use aethereal_substrate::Engine;
use aethereal_window::{Heard, Opening, Windows};
use core::num::NonZeroUsize;
use js_sys::Math;
use wasm_bindgen::JsError;
use wasm_bindgen::prelude::wasm_bindgen;
use web_sys::HtmlCanvasElement;

/// The program the page runs, for its `requestAnimationFrame` to call, and the page's listeners, kept while it runs.
#[wasm_bindgen]
pub struct Program {
    browser: Browser<Performance>,
    _canvas: Listening,
}

/// Opens `canvas` as the one window and WebGPU on it, then makes the engine with the window root, the renderer, handed
/// the GPU, and the game as its roots, the match's chance seeded from the page's, and listens to the page for the
/// keys, the focus, and resizes.
///
/// # Errors
///
/// When WebGPU will not open on the canvas, a root does not begin, or the page will not be listened to.
#[wasm_bindgen]
pub async fn start(canvas: HtmlCanvasElement) -> Result<Program, JsError> {
    let mut setup = Engine::new(NonZeroUsize::MIN, 1024, 1 << 20, 1 << 20, 0)
        .with_cycle::<(Tick, Render)>()
        .map_err(|unordered| JsError::new(&format!("the cycle is out of order: {unordered:?}")))?;
    let mut opening = Opening::new(setup.source::<Heard>());
    let (gpu, shown) = open(canvas.clone(), &mut opening).await?;
    let mut setup = setup
        .add_root::<Windows>(&(), opening.config())
        .map_err(|refused| JsError::new(&format!("the window root did not begin: {refused:?}")))?;
    let (rendering, mappings) = renderer(&mut setup, gpu);
    let game = Match { window: shown.window(), seed: Math::random().to_bits() };
    let engine = setup
        .add_root::<Renderer>(&(), rendering)
        .map_err(|refused| JsError::new(&format!("the renderer did not begin: {refused:?}")))?
        .add_root::<Pong>(&(), game)
        .map_err(|refused| JsError::new(&format!("the game did not begin: {refused:?}")))?
        .ready();
    let program =
        Program { browser: Browser::new(engine, Performance, mappings), _canvas: canvas::listen(&canvas, &shown)? };

    Ok(program)
}

#[wasm_bindgen]
impl Program {
    /// Runs one frame, at the timestamp `requestAnimationFrame` handed its callback, in milliseconds.
    pub fn frame(&mut self, timestamp_millis: f64) {
        self.browser.frame(timestamp_millis);
    }
}
