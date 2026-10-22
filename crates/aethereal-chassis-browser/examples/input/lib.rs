//! The page's input: a root that subscribes to the canvas's window and to the controls, and shows each event and
//! control it is sent: the pointer and the wheel over the canvas in the canvas's pixels, the keys, the focus, resizes,
//! and a slider the page marks. `build.sh` beside it builds the page; `index.html` runs it.

#[cfg(target_arch = "wasm32")]
mod page;
