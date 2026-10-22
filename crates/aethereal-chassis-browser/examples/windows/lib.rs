//! Two canvases on a page, each a window of its own, `0` and `1`, each drawing its own main frame: a ring of
//! triangles of its own count and colours, following the pointer over that canvas. Each resize of a canvas is said
//! beside its window. `build.sh` beside it builds the page; `index.html` runs it.

#[cfg(target_arch = "wasm32")]
mod page;
