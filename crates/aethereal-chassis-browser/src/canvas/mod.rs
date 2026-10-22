//! Each canvas as a window of the window capability (ADR-0024): opened as the program is set up, its surface its
//! backing store, drawn at the size the page lays it out at, and fed what the page hears over it, the pointer and the
//! wheel in its pixels, and the keys, the focus, and the resizes of the page.

#[cfg(any(target_arch = "wasm32", test))]
mod dom;
#[cfg(target_arch = "wasm32")]
mod listen;
#[cfg(test)]
mod tests;

#[cfg(target_arch = "wasm32")]
pub use dom::Density;
#[cfg(target_arch = "wasm32")]
pub use listen::{Page, Shown, listen};
