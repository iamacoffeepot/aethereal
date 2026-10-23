//! Soft rims laid over a floor, on a page: a floor drawn into a target of floats of one sample, writing its depth; rims
//! drawn into a target of floats of four samples with a depth of their own, each rim's alpha its coverage, reading the
//! floor's depth to drop what the floor is in front of; and an effect that lays the rims, resolved, on the floor onto
//! the canvas. The root makes what it draws and the frame, and places the floor and the rims.
//! `cargo xtask example rims --serve` builds the page and serves it.

#[cfg(target_arch = "wasm32")]
mod page;
#[cfg(target_arch = "wasm32")]
#[path = "../shared/said.rs"]
mod said;
