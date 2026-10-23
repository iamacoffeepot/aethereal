//! A frame of four passes, on a page: a sky drawn into a target of floats by an effect that reads nothing, surfaces
//! drawn over it writing depth, splats drawn over them tested against that depth, and a tint over all three onto the
//! canvas, an effect that reads the scene. The root makes what it draws, the frame, and places a surface and a splat,
//! then reads back what was drawn and times the frame's passes. `cargo xtask example frames --serve` builds the page
//! and serves it.

#[cfg(target_arch = "wasm32")]
mod page;
#[cfg(target_arch = "wasm32")]
#[path = "../shared/said.rs"]
mod said;
