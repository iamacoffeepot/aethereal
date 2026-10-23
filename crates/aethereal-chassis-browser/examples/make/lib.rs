//! The renderer on a page: a root that, as the program starts, asks the renderer at its typed path for a triangle, a
//! texture, and a triangle it refuses, and shows what each was answered. `build.sh make` beside the examples builds the
//! page.

#[cfg(target_arch = "wasm32")]
mod page;
#[cfg(target_arch = "wasm32")]
#[path = "../shared/said.rs"]
mod said;
