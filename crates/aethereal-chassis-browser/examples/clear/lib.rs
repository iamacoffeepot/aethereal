//! The browser chassis stood up: one root that clears the page's canvas, each frame, to a colour that drifts with the
//! program's time. `build.sh` beside it builds the page; `index.html` runs it.

#[cfg(target_arch = "wasm32")]
mod page;
