//! Reading back what is drawn, on a page: a root that places two squares, their places and colours read from storage,
//! and a tint that multiplies the left half of the canvas, then reads back a frame, a texture it made, and how long
//! a frame's pass took, and shows what it was answered. `build.sh` beside it builds the page; `index.html` runs it.

#[cfg(target_arch = "wasm32")]
mod page;
