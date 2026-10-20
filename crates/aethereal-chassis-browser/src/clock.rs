//! The page's clock, handed to the engine (ADR-0001).

use aethereal_substrate::Clock;
use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen]
extern "C" {
    /// Milliseconds since the page was opened, to a fraction: `performance.now()`.
    #[wasm_bindgen(js_namespace = performance, js_name = now)]
    fn performance_now() -> f64;
}

/// The page's monotonic clock, `performance.now()`, which reads zero when the page was opened. Animation frames are
/// timed on the same clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct Performance;

impl Clock for Performance {
    fn nanos(&self) -> u64 {
        // The page's clock reads milliseconds, never negative, and a u64 of nanoseconds holds centuries of them.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let nanos = (performance_now() * 1_000_000.0) as u64;

        nanos
    }
}
