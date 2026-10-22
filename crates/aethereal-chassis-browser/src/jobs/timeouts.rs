//! The page's timer, which the jobs that wait wait on: `setTimeout`, read from `performance.now()` (ADR-0018).

use crate::Performance;
use aethereal_actor::{Time, Timer};
use aethereal_substrate::Clock;
use std::task::Waker;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen]
extern "C" {
    /// Calls `handler` once, `millis` milliseconds from now or as soon as the page can after: `setTimeout`.
    #[wasm_bindgen(js_name = setTimeout)]
    fn set_timeout(handler: &JsValue, millis: f64);
}

/// The page's timer, on the clock its frames are timed by, so a job that waits until a step's time plus some time
/// waits about that much of the page's time. It holds nothing: each wait is a timeout of its own.
pub(super) struct Timeouts;

impl Timer for Timeouts {
    fn now(&self) -> Time {
        Time::from_nanos(Performance.nanos())
    }

    /// Wakes `waker` once the page's timeout for `at` runs out, the milliseconds to it rounded up. A timeout longer
    /// than the page keeps, some 24 days, wakes it at that, and it waits again.
    fn wake_at(&self, at: Time, waker: Waker) {
        let nanos = at.nanos().saturating_sub(self.now().nanos());
        let millis = i32::try_from(nanos.div_ceil(1_000_000)).unwrap_or(i32::MAX);
        set_timeout(&Closure::once_into_js(move || waker.wake()), f64::from(millis));
    }
}
