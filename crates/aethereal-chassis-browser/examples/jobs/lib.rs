//! Jobs on a page: a root that, once it has made what it draws, runs a sleep of a second, a small sum to compute, a
//! sleep it gives up on, and a sleep of three seconds to check by, and places a square on the canvas as each is
//! answered: the sum's on the left, the second's in the middle, and on the right one that says the sleep given up on
//! was never answered. `cargo xtask example jobs --serve` builds the page and serves it.

#[cfg(target_arch = "wasm32")]
mod page;
#[cfg(target_arch = "wasm32")]
#[path = "../shared/said.rs"]
mod said;
