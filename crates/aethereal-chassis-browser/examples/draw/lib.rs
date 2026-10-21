//! What is drawn, on a page: a root that makes a triangle, a program that reads a place and a colour per instance,
//! instances, and uniforms, places the triangle once for each instance, and at each `Tick` writes where each is and how
//! many there are, and how large they are drawn. `build.sh` beside it builds the page; `index.html` runs it.

#[cfg(target_arch = "wasm32")]
mod page;
