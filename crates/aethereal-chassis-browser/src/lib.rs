//! The browser chassis: the engine on a page's one thread, cycled by its animation frames (ADR-0001, ADR-0020).
//!
//! The page owns the thread, its canvases, and when a frame is drawn. It
//! opens the GPU on the canvases it hands over with [`open`] before the engine
//! is made, each canvas a window, hands it in to the renderer with
//! [`renderer`] (ADR-0010, ADR-0021), and from each `requestAnimationFrame`
//! calls [`Browser::frame`], which hands the renderer each canvas's frame,
//! runs one cycle through to its end before the page gets its thread back,
//! presents what the renderer drew on each, then starts mapping what it asked
//! to read back, which the page's promises bring in later frames.
//!
//! What the page hears, its callbacks push onto a source as they run, and the
//! next frame brings in (ADR-0010): the pointer over a canvas, the keys, the
//! focus, and resizes onto the window capability's, each canvas a window
//! ([`canvas`], ADR-0024), and the controls the page marks onto the
//! [`controls`] root's.
//!
//! A page owns its canvases, and the engine in it cannot make one: each window
//! an actor asks for while the program runs, and each it asks to close, is
//! answered `NoHost`, between steps ([`Hosted`], ADR-0027). A page that wants
//! another window hands the chassis another canvas as it sets the program up.
//!
//! The jobs its turns run, it runs on the page's own event loop, each a task of
//! its own, with `EventLoop` handed to the engine's setup: their answers are
//! brought in a later frame (ADR-0018).
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
mod hosted;
#[cfg(any(target_arch = "wasm32", test))]
mod jobs;
#[cfg(target_arch = "wasm32")]
mod listening;
mod serving;
#[cfg(test)]
mod tests;

#[cfg(target_arch = "wasm32")]
pub use clock::Performance;
#[cfg(target_arch = "wasm32")]
pub use gpu::{Opened, open, renderer};
pub use hosted::Hosted;
#[cfg(target_arch = "wasm32")]
pub use jobs::EventLoop;
#[cfg(target_arch = "wasm32")]
pub use listening::Listening;
pub use serving::{Drawing, Serving};
/// The futures crate an async `#[wasm_bindgen]` export names, for a page to point the macro at.
#[cfg(target_arch = "wasm32")]
pub use wasm_bindgen_futures;

use aethereal_actor::Time;
use aethereal_substrate::{Clock, Engine};

/// An engine run on the page's thread, timed by the clock `C`: the page's own, [`Performance`], in a browser, with
/// what it serves the renderer and what it answers the window root with.
pub struct Browser<C> {
    engine: Engine,
    clock: C,
    serving: Serving,
    hosted: Hosted,
}

impl<C: Clock> Browser<C> {
    /// `engine`, with its roots made, started on `clock`, serving its renderer as `serving` says, and answering its
    /// window root through `hosted`: its `Start` runs in the first frame (ADR-0020).
    #[must_use]
    pub fn new(mut engine: Engine, clock: C, serving: Serving, hosted: Hosted) -> Self {
        engine.start();

        Self { engine, clock, serving, hosted }
    }

    /// The engine, as of the last frame.
    #[must_use]
    pub const fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Runs one frame, for the page's `requestAnimationFrame`, whose callback is handed the frame's timestamp,
    /// `timestamp_millis`, in milliseconds since the page opened. First it hands the renderer each canvas's frame,
    /// its surface configured to the size its window was last resized to. Then it says a cycle is due at that time,
    /// so `Tick`'s delta is the time between the display's frames however long each frame's work took, and brings in
    /// what its sources were pushed and its jobs finished and steps, until no actor has anything to run (ADR-0010,
    /// ADR-0018), answering what the window root asked between steps, `NoHost` (ADR-0027). The cycle's stages each run
    /// through to their end within the call, so what `Render` drew is
    /// presented before the page gets its thread back and shows it. Then it starts mapping what the renderer asked to
    /// read back, its turns' copies submitted, for the page's promises to bring in once the GPU has mapped it
    /// (ADR-0019).
    ///
    /// # Panics
    ///
    /// Panics with a turn's panic, and when a turn makes an actor under an id the table already holds (ADR-0008).
    pub fn frame(&mut self, timestamp_millis: f64) {
        self.serving.acquire();
        self.engine.cycle(frame_time(timestamp_millis));
        self.hosted.refuse();
        self.engine.bring();
        while !self.engine.is_idle() {
            let now = Time::from_nanos(self.clock.nanos());
            self.engine.step(now, &self.clock);
            self.hosted.refuse();
            self.engine.bring();
        }
        self.serving.serve();
    }
}

/// The time of an animation frame whose timestamp is `millis`, in milliseconds since the page opened, as the page's
/// clock, `performance.now()`, counts it: none before the page opened.
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a page is not open for 584 years")]
fn frame_time(millis: f64) -> Time {
    Time::from_nanos((millis.max(0.0) * 1_000_000.0) as u64)
}
