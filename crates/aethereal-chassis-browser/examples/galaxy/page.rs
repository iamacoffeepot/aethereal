//! The page's entry points: the engine made with the window root, the renderer, the disc, and the lens as its roots,
//! and the pointer over the canvas listened to for the disc.

use crate::disc::{Disc, STARS};
use crate::lens::Lens;
use crate::mail::Canvas;
use aethereal_actor::{Render, Tick};
use aethereal_chassis_browser::{Browser, Listening, Performance, canvas, open, renderer};
use aethereal_render::Renderer;
use aethereal_substrate::Engine;
use aethereal_window::{Heard, Opening, Windows};
use core::cell::RefCell;
use core::mem;
use core::num::NonZeroUsize;
use wasm_bindgen::JsError;
use wasm_bindgen::prelude::wasm_bindgen;
use web_sys::HtmlCanvasElement;

thread_local! {
    /// What went wrong as the roots made what they draw, for the page to show.
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Notes what went wrong as a root made what it draws.
pub fn say(line: String) {
    SAID.with_borrow_mut(|said| said.push(line));
}

/// The program the page runs, for its `requestAnimationFrame` to call, and the page's listeners, kept while it runs.
#[wasm_bindgen]
pub struct Program {
    browser: Browser<Performance>,
    _canvas: Listening,
}

/// Opens `canvas` as the one window and WebGPU on it, then makes the engine with the window root, the renderer, handed
/// the GPU, the disc, and the lens as its roots, each told the canvas's size, and listens to the page for the pointer
/// over the canvas. Its blobs hold the stars as they begin.
///
/// # Errors
///
/// When WebGPU will not open on the canvas, a root does not begin, or the page will not be listened to.
#[wasm_bindgen]
pub async fn start(canvas: HtmlCanvasElement) -> Result<Program, JsError> {
    let mut setup = Engine::new(NonZeroUsize::MIN, 1024, 1 << 20, 1 << 26, 0)
        .with_cycle::<(Tick, Render)>()
        .map_err(|unordered| JsError::new(&format!("the cycle is out of order: {unordered:?}")))?;
    let mut opening = Opening::new(setup.source::<Heard>());
    let (gpu, shown) = open(canvas.clone(), &mut opening).await?;
    let mut setup = setup
        .add_root::<Windows>(&(), opening.config())
        .map_err(|refused| JsError::new(&format!("the window root did not begin: {refused:?}")))?;
    let (rendering, mappings) = renderer(&mut setup, gpu);
    let sized = Canvas { window: shown.window(), width: canvas.width(), height: canvas.height() };
    // The lens asks the disc at its typed path, so the disc is made first.
    let engine = setup
        .add_root::<Renderer>(&(), rendering)
        .map_err(|refused| JsError::new(&format!("the renderer did not begin: {refused:?}")))?
        .add_root::<Disc>(&(), sized)
        .map_err(|refused| JsError::new(&format!("the disc did not begin: {refused:?}")))?
        .add_root::<Lens>(&(), sized)
        .map_err(|refused| JsError::new(&format!("the lens did not begin: {refused:?}")))?
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

    /// How many stars the galaxy holds.
    #[must_use]
    #[expect(clippy::unused_self, reason = "the page asks it of the program it started")]
    pub fn stars(&self) -> u32 {
        STARS
    }

    /// What went wrong since this was last asked, a line each.
    #[must_use]
    #[expect(clippy::unused_self, reason = "the page asks it of the program it started")]
    pub fn said(&self) -> Vec<String> {
        SAID.with_borrow_mut(mem::take)
    }
}
