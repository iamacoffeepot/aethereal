//! Pong, on a page: the player's paddle on the left, moved by the arrow keys or W and S, against an opponent on the
//! right that plays as its own actor. The game is a root that keeps the match and plays the ball at each `Tick`; beneath
//! it the screen draws the court the game sends it, and the opponent steers its paddle by mail from the same court.
//! `build.sh` beside it builds the page; `index.html` runs it.

#[cfg(target_arch = "wasm32")]
mod court;
#[cfg(target_arch = "wasm32")]
mod dice;
#[cfg(target_arch = "wasm32")]
mod flight;
#[cfg(target_arch = "wasm32")]
mod game;
#[cfg(target_arch = "wasm32")]
mod opponent;
#[cfg(target_arch = "wasm32")]
mod page;
#[cfg(target_arch = "wasm32")]
mod screen;
