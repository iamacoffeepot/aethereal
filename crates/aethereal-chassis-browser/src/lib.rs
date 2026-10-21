//! The browser chassis: the engine on a page's one thread, cycled by its animation frames (ADR-0001, ADR-0020).
//!
//! The page owns the thread, the canvas, and when a frame is drawn. It opens
//! the GPU with [`open`] before the engine is made, hands it in to the
//! renderer (ADR-0010, ADR-0021), and from each `requestAnimationFrame` calls
//! [`Browser::frame`], which runs one cycle through to its end before the
//! page gets its thread back, then starts mapping what the renderer asked
//! to read back, which the page's promises bring in later frames.
//!
//! What the page hears, its callbacks push onto a source as they run, and the
//! next frame brings in (ADR-0010): the pointer over the canvas, the keys, the
//! focus, and resizes onto the window capability's, the canvas its one window
//! ([`canvas`], ADR-0024), and the controls the page marks onto the
//! [`controls`] root's.
//!
//! A page exports an async `start` that opens the GPU, and `#[wasm_bindgen]`
//! on an async function writes code that names `wasm_bindgen_futures`. The
//! chassis re-exports that crate, so a page points the macro at it through
//! the chassis rather than depending on it, as the `draw` example does:
//!
//! ```ignore
//! #[wasm_bindgen(wasm_bindgen_futures = aethereal_chassis_browser::wasm_bindgen_futures)]
//! pub async fn start(canvas: HtmlCanvasElement) -> Result<Program, JsError> { /* ... */ }
//! ```
//!
//! The page still depends on `wasm-bindgen` itself, for the macro, at the
//! version the chassis pins: the `wasm-bindgen` tool's that writes its glue.

pub mod canvas;
#[cfg(target_arch = "wasm32")]
mod clock;
pub mod controls;
#[cfg(target_arch = "wasm32")]
mod gpu;
#[cfg(target_arch = "wasm32")]
mod listening;
mod mappings;
#[cfg(test)]
mod tests;

#[cfg(target_arch = "wasm32")]
pub use clock::Performance;
#[cfg(target_arch = "wasm32")]
pub use gpu::open;
#[cfg(target_arch = "wasm32")]
pub use listening::Listening;
pub use mappings::{Mappings, renderer};
/// The futures crate an async `#[wasm_bindgen]` export names, for a page to point the macro at.
#[cfg(target_arch = "wasm32")]
pub use wasm_bindgen_futures;

use aethereal_actor::Time;
use aethereal_substrate::{Clock, Engine};

/// An engine run on the page's thread, timed by the clock `C`: the page's own, [`Performance`], in a browser, with
/// what it serves the renderer.
pub struct Browser<C> {
    engine: Engine,
    clock: C,
    mappings: Mappings,
}

impl<C: Clock> Browser<C> {
    /// `engine`, with its roots made, started on `clock`, serving the renderer `mappings`: its `Start` runs in the
    /// first frame (ADR-0020).
    #[must_use]
    pub fn new(mut engine: Engine, clock: C, mappings: Mappings) -> Self {
        engine.start();

        Self { engine, clock, mappings }
    }

    /// The engine, as of the last frame.
    #[must_use]
    pub const fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Runs one frame, for the page's `requestAnimationFrame`: says a cycle is due, then brings in what its sources
    /// were pushed and its jobs finished and steps, until no actor has anything to run (ADR-0010, ADR-0018). The
    /// cycle's stages each run through to their end within the call, so what `Render` drew is there when the page
    /// gets its thread back and shows it. Then it starts mapping what the renderer asked to read back, its turns'
    /// copies submitted, for the page's promises to bring in once the GPU has mapped it (ADR-0019).
    ///
    /// # Panics
    ///
    /// Panics with a turn's panic, and when a turn makes an actor under an id the table already holds (ADR-0008).
    pub fn frame(&mut self) {
        self.engine.cycle();
        self.engine.bring();
        while !self.engine.is_idle() {
            let now = Time::from_nanos(self.clock.nanos());
            self.engine.step(now, &self.clock);
            self.engine.bring();
        }
        self.mappings.serve();
    }
}
