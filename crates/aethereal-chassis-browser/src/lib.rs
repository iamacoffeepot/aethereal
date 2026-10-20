//! The browser chassis: the engine on a page's one thread, cycled by its animation frames (ADR-0001, ADR-0020).
//!
//! The page owns the thread, the canvas, and when a frame is drawn. It opens
//! the [`Gpu`] before the engine is made, hands it in to the root that draws
//! (ADR-0010), and from each `requestAnimationFrame` calls
//! [`Browser::frame`], which runs one cycle through to its end before the
//! page gets its thread back.

#[cfg(target_arch = "wasm32")]
mod clock;
#[cfg(target_arch = "wasm32")]
mod gpu;
#[cfg(test)]
mod tests;

#[cfg(target_arch = "wasm32")]
pub use clock::Performance;
#[cfg(target_arch = "wasm32")]
pub use gpu::Gpu;

use aethereal_actor::Time;
use aethereal_substrate::{Clock, Engine};

/// An engine run on the page's thread, timed by the clock `C`: the page's own, [`Performance`], in a browser.
pub struct Browser<C> {
    engine: Engine,
    clock: C,
}

impl<C: Clock> Browser<C> {
    /// `engine`, with its roots made, started on `clock`: its `Start` runs in the first frame (ADR-0020).
    #[must_use]
    pub fn new(mut engine: Engine, clock: C) -> Self {
        engine.start();

        Self { engine, clock }
    }

    /// The engine, as of the last frame.
    #[must_use]
    pub const fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Runs one frame, for the page's `requestAnimationFrame`: says a cycle is due, then brings in what its sources
    /// were pushed and its jobs finished and steps, until no actor has anything to run (ADR-0010, ADR-0018). The
    /// cycle's stages each run through to their end within the call, so what `Render` drew is there when the page
    /// gets its thread back and shows it.
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
    }
}
