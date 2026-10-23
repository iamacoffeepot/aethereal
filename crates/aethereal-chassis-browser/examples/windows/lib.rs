//! Two canvases on a page, each a window of its own, `0` and `1`, each drawing its own main frame: a ring of triangles
//! of its own count and colours, following the pointer over that canvas. Each resize of a canvas is said beside its
//! window. `N` asks the window root for another window, which a page answers `NoHost`, said as `refused open
//! asked_by=<window> why=NoHost`. `cargo xtask example windows --serve` builds the page and serves it.

#[cfg(target_arch = "wasm32")]
mod page;
#[cfg(target_arch = "wasm32")]
#[path = "../shared/said.rs"]
mod said;
