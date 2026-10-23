//! A frame that ends on a texture, on a page: a map drawn into a texture of its own, which the window's main frame,
//! made by the same root, draws on a quad. A second root asks to replace the window's main frame and is refused, naming
//! its owner. The map is read back before and after it is released, as the main frame keeps drawing it as it was last
//! drawn. A third root keeps a glow of 2.5 in a frame that ends in floats, which another frame's effect samples and
//! darkens by a quarter into bytes, and reads both back. `cargo xtask example texture --serve` builds the page and
//! serves it.

#[cfg(target_arch = "wasm32")]
mod page;
#[cfg(target_arch = "wasm32")]
#[path = "../shared/said.rs"]
mod said;
