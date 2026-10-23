//! Compute, on a page: a field of particles held in storage, moved at every `Render` by a dispatch in the renderer's
//! own main frame's compute pass, and drawn from the same storage by a placement in its pass that draws. The root makes
//! what it runs and draws, then reads the storage back twice, a few frames apart, and shows whether the GPU moved the
//! particles, with how long each of the frame's passes took. `cargo xtask example compute --serve` builds the page and
//! serves it.

#[cfg(target_arch = "wasm32")]
mod page;
#[cfg(target_arch = "wasm32")]
#[path = "../shared/said.rs"]
mod said;
