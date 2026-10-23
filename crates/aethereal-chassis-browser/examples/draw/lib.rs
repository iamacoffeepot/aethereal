//! What is drawn, on a page: a root that makes a triangle, a program that reads a place and a colour per instance,
//! instances, and uniforms, places the triangle once for each instance, and at each `Tick` writes where each is and how
//! many there are, and how large they are drawn. `build.sh draw` beside the examples builds the page.

#[cfg(target_arch = "wasm32")]
mod page;
#[cfg(target_arch = "wasm32")]
#[path = "../shared/said.rs"]
mod said;
