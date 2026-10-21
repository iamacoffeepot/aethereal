//! A frame that ends on a texture, on a page: a map drawn into a texture of its own, which the window's main frame,
//! made by the same root, draws on a quad. A second root asks to replace the window's main frame and is refused,
//! naming its owner. The map is read back before and after it is released, as the main frame keeps drawing it as it
//! was last drawn. `build.sh` beside it builds the page; `index.html` runs it.

#[cfg(target_arch = "wasm32")]
mod page;
