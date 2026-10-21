//! The page's input: a root that subscribes to the input root and shows each event it is sent, the pointer over the
//! canvas in the canvas's pixels, the keys, and a slider the page marks. `build.sh` beside it builds the page;
//! `index.html` runs it.

#[cfg(target_arch = "wasm32")]
mod page;
