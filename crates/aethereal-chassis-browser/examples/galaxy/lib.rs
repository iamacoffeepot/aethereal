//! A galaxy, on a page: sixty-five thousand stars in a spiral-armed disc, moved each frame by a compute program in
//! storage the GPU alone writes, drawn as soft sprites that add their light into a target of floats, and given a glow
//! by a bloom of its own frame: the stars drawn again small, the bright part kept, and blurred across then down. The
//! window's frame lays the glow over the stars, maps the light to the screen with a filmic curve, and darkens the
//! corners. Held down, the pointer pulls the stars near it, which stream back into the arms once it lets go.
//!
//! Two roots draw it, talking only by mail: the disc seeds the stars, moves them, and feeds the pointer to the GPU,
//! and the lens makes the frames that simulate, glow, and show them, and places the stars in them. `build.sh` beside
//! it builds the page; `index.html` runs it.

#[cfg(target_arch = "wasm32")]
mod bytes;
#[cfg(target_arch = "wasm32")]
mod camera;
#[cfg(target_arch = "wasm32")]
mod disc;
#[cfg(target_arch = "wasm32")]
mod lens;
#[cfg(target_arch = "wasm32")]
mod mail;
#[cfg(target_arch = "wasm32")]
mod page;
#[cfg(target_arch = "wasm32")]
mod passes;
