//! The page's memory, as the browser reports it, for the engine's figures (ADR-0053).
//!
//! A browser tells a page no resident figure. Chromium's `performance.memory` reports the script heap the page uses;
//! other browsers report nothing, and the figure is unknown.

use aethereal_actor::Process;
use js_sys::{Reflect, global};
use wasm_bindgen::JsValue;

/// The script heap the page uses, `performance.memory.usedJSHeapSize`, where the browser reports one: unknown where it
/// does not.
#[must_use]
pub fn heap() -> Process {
    used_heap().map_or(Process::Unknown, |bytes| Process::Heap { bytes })
}

/// The bytes of `performance.memory.usedJSHeapSize`: none when the browser has no such field, or it is not a number.
fn used_heap() -> Option<u64> {
    let performance = Reflect::get(&global(), &JsValue::from_str("performance")).ok()?;
    let memory = Reflect::get(&performance, &JsValue::from_str("memory")).ok()?;
    let used = Reflect::get(&memory, &JsValue::from_str("usedJSHeapSize")).ok()?.as_f64()?;
    // The browser counts bytes in a double, never negative, which a u64 holds whole well past any page's heap.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let bytes = used as u64;

    Some(bytes)
}
