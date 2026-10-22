//! What an example's page shows over its canvas: the lines its roots say, and the program its `requestAnimationFrame`
//! runs and asks for them. Every example's `lib.rs` includes this one file.

use aethereal_chassis_browser::Started;
use core::cell::RefCell;
use core::mem;
use wasm_bindgen::prelude::wasm_bindgen;

thread_local! {
    /// What the example's roots said, for the page to show.
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Notes a line for the page to show.
pub fn say(line: String) {
    SAID.with_borrow_mut(|said| said.push(line));
}

/// The program the page runs, for its `requestAnimationFrame` to call.
#[wasm_bindgen]
pub struct Program {
    started: Started,
}

impl From<Started> for Program {
    fn from(started: Started) -> Self {
        Self { started }
    }
}

#[wasm_bindgen]
impl Program {
    /// Runs one frame, at the timestamp `requestAnimationFrame` handed its callback, in milliseconds.
    pub fn frame(&mut self, timestamp_millis: f64) {
        self.started.frame(timestamp_millis);
    }

    /// What the example's roots said since this was last asked, a line each.
    #[must_use]
    #[expect(clippy::unused_self, reason = "the page asks it of the program it started")]
    pub fn said(&self) -> Vec<String> {
        SAID.with_borrow_mut(mem::take)
    }
}
