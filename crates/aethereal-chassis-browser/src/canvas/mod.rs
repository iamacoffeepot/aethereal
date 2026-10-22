//! The canvas as the one window of the window capability (ADR-0024): opened as the program is set up, its surface its
//! backing store, and fed what the page hears over it, the pointer and the wheel in its pixels, and the keys, the
//! focus, and the resizes of the page.

#[cfg(any(target_arch = "wasm32", test))]
mod dom;
#[cfg(target_arch = "wasm32")]
mod listen;
#[cfg(test)]
mod tests;

#[cfg(target_arch = "wasm32")]
pub use listen::{listen, open};
