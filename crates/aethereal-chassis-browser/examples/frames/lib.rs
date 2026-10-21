//! A frame of three passes, on a page: surfaces drawn into a target of floats writing depth, splats drawn over them
//! tested against that depth, and a tint over both onto the canvas, an effect. The root makes what it draws, the
//! frame, and places a surface and a splat, then reads back what was drawn and times the frame's passes. `build.sh`
//! beside it builds the page; `index.html` runs it.

#[cfg(target_arch = "wasm32")]
mod page;
