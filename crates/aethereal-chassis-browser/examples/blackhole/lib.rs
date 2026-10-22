//! A Schwarzschild black hole, on a page, traced through the curved spacetime around it one photon a pixel. Each
//! pixel's light is followed backward from the camera along a null geodesic, falls into the hole or bends around it,
//! and lands on the thin accretion disk or on the sky. The disk glows at the Novikov–Thorne temperature, shifted by
//! the redshift of the gravity it climbs out of and the Doppler shift of the gas's orbit, and its far side is lensed
//! over and under the shadow. A glow is bloomed from the bright parts in a frame of its own, and the window's frame
//! lays it over the light and maps it to the screen with a filmic curve. Dragged, the camera orbits the hole; left
//! alone, it drifts.
//!
//! Two roots draw it, talking only by mail: the orbit keeps the camera, following the pointer and the clock, and
//! writes its view into uniforms the GPU reads, and the lens makes the frames that trace and show the hole, and
//! places the photons' quad in them, bound to the orbit's view. `build.sh` beside it builds the page; `index.html`
//! runs it.

#[cfg(target_arch = "wasm32")]
mod bytes;
#[cfg(target_arch = "wasm32")]
mod lens;
#[cfg(target_arch = "wasm32")]
mod mail;
#[cfg(target_arch = "wasm32")]
mod orbit;
#[cfg(target_arch = "wasm32")]
mod page;
#[cfg(target_arch = "wasm32")]
mod passes;
