//! The desktop chassis: the native host with windows, on winit, drawing through the GPU it opens on them (ADR-0001,
//! ADR-0024).
//!
//! **A native-only host crate** (ADR-0001): native windows, their GPU, and the
//! waits on them are a native host's, and a browser has the browser chassis.
//! A build for `wasm32` compiles it empty.
//!
//! It runs the engine on the thread of winit's event loop, which steps it as
//! the first worker of the headless chassis's pool, the others its own
//! threads (ADR-0011). [`Desktop`] is a program's host (ADR-0029): inside the
//! loop's first callback it bootstraps the program on the [`Screen`], where
//! the window root's bootstrap opens each window its settings list, as a
//! window of the window capability, and the renderer's opens the GPU on the
//! first, each waited for on that thread, which is the host's to do (ADR-0001
//! rule 4). The window root gives the desktop its end of the root's
//! controller, and the renderer gives it what the desktop serves it each
//! frame, [`Drawing`].
//!
//! Each frame it acquires the surface of each window shown, in turn, says a
//! cycle is due, steps until no actor has anything to run, then presents
//! what the renderer drew in each window and starts mapping what it asked to
//! read back (ADR-0020, ADR-0021). The renderer's settings say what paces the
//! frames, as [`Presenting`] each window as it is opened: `fifo`, the
//! default, where each acquire waits for the display to free an image, so a
//! window on a slower display paces them all to its rate; `uncapped`, where
//! none waits and each frame follows the last; or `capped`, where none waits
//! and the loop waits until each frame is due on a grid of the rate, skipping
//! one missed as the headless host skips a cycle.
//! A window hidden or of no size is handed no frame, and when none is shown
//! the loop waits on their events until one is shown again. What a window
//! hears is pushed onto that window's feed as winit hears it, in the window's
//! surface pixels, routed by winit's id for the window, and brought in as the
//! engine next steps (ADR-0024).
//!
//! Between steps it takes what the window root asked for, and opens each
//! window asked for, its surface on the same GPU and its slot in the
//! renderer's front, or closes it, telling the renderer and then the window
//! root (ADR-0027). A window's close button closes it unless the program says
//! it only asks, and once its last window has closed the program is asked to
//! stop, as `quit` does, and `Stop` is given the time the host gives it to
//! finish.

#[cfg(not(target_arch = "wasm32"))]
mod gpu;
#[cfg(not(target_arch = "wasm32"))]
mod input;
#[cfg(not(target_arch = "wasm32"))]
mod presenting;
#[cfg(not(target_arch = "wasm32"))]
mod program;
#[cfg(not(target_arch = "wasm32"))]
mod running;
#[cfg(not(target_arch = "wasm32"))]
mod serving;

#[cfg(not(target_arch = "wasm32"))]
pub use gpu::{Screen, Unopened};
#[cfg(not(target_arch = "wasm32"))]
pub use presenting::{FrameRate, Presenting, Unoffered, Unrated};
#[cfg(not(target_arch = "wasm32"))]
pub use program::{RendererSettings, RunningSettings, WindowSettings, WindowsSettings};
#[cfg(not(target_arch = "wasm32"))]
pub use running::{Desktop, Failed};
#[cfg(not(target_arch = "wasm32"))]
pub use serving::Drawing;
