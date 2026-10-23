//! The page's input: a root that subscribes to the canvas's window and to the controls, and shows each event and
//! control it is sent: the pointer and the wheel over the canvas in the canvas's pixels, the keys, the focus, resizes,
//! and a slider the page marks. `cargo xtask example input --serve` builds the page and serves it.

#[cfg(target_arch = "wasm32")]
mod page;
#[cfg(target_arch = "wasm32")]
#[path = "../shared/said.rs"]
mod said;
