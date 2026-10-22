//! The desktop chassis: the native host with windows, on winit, drawing through the GPU it opens on them (ADR-0001,
//! ADR-0024).
//!
//! **A native-only host crate** (ADR-0001): native windows, their GPU, and the
//! waits on them are a native host's, and a browser has the browser chassis.
//! A build for `wasm32` compiles it empty.
//!
//! It runs the engine on the thread of winit's event loop, which steps it as
//! the first worker of the headless chassis's pool, the others its own
//! threads (ADR-0011). [`Desktop::run`] hands the program the [`Screen`] as
//! the loop begins, and the program opens each window it asks for with
//! [`Screen::open_window`], as a window of the window capability, the first
//! opening the GPU on it, each waited for on that thread, which is the host's
//! to do (ADR-0001 rule 4). It then makes the renderer's config with
//! [`renderer`], as a page does in a browser.
//!
//! Each frame it acquires the surface of each window shown, in turn, which
//! with `Fifo` waits for the display to free an image and so paces the
//! frames, says a cycle is due, steps until no actor has anything to run,
//! then presents what the renderer drew in each window and starts mapping
//! what it asked to read back (ADR-0020, ADR-0021). One cycle draws every
//! window shown, so a window on a slower display paces them all to its rate.
//! A window hidden or of no size is handed no frame, and when none is shown
//! the loop waits on their events until one is shown again. What a window
//! hears is pushed onto that window's feed as winit hears it, in the window's
//! surface pixels, routed by winit's id for the window, and brought in as the
//! engine next steps (ADR-0024). The close button of the program's one window
//! asks it to stop, as `quit` does, and `Stop` is given the time the host
//! gives it to finish; with several windows, it only asks the program.

#[cfg(not(target_arch = "wasm32"))]
mod gpu;
#[cfg(not(target_arch = "wasm32"))]
mod input;
#[cfg(not(target_arch = "wasm32"))]
mod running;
#[cfg(not(target_arch = "wasm32"))]
mod serving;

#[cfg(not(target_arch = "wasm32"))]
pub use gpu::{Screen, Unopened, renderer};
#[cfg(not(target_arch = "wasm32"))]
pub use running::{Desktop, Failed, Made};
#[cfg(not(target_arch = "wasm32"))]
pub use serving::Drawing;
