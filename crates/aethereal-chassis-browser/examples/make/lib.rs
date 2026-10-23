//! The renderer on a page: a root that, as the program starts, asks the renderer at its typed path for a triangle, a
//! texture, and a triangle it refuses, and shows what each was answered. `cargo xtask example make --serve` builds the
//! page and serves it.

#[cfg(target_arch = "wasm32")]
mod page;
#[cfg(target_arch = "wasm32")]
#[path = "../shared/said.rs"]
mod said;
