//! Soft rims laid over a floor, on a page: a floor drawn into a target of floats of one sample, writing its depth;
//! rims drawn into a target of floats of four samples with a depth of their own, each rim's alpha its coverage, reading
//! the floor's depth to drop what the floor is in front of; and an effect that lays the rims, resolved, on the floor
//! onto the canvas. The root makes what it draws and the frame, and places the floor and the rims. `build.sh` beside it
//! builds the page; `index.html` runs it.

#[cfg(target_arch = "wasm32")]
mod page;
