//! The desktop chassis: the native host with a window, on winit, drawing through the GPU it opens on it (ADR-0001,
//! ADR-0024).
//!
//! **A native-only host crate** (ADR-0001): a native window, its GPU, and the
//! waits on them are a native host's, and a browser has the browser chassis.
//! A build for `wasm32` compiles it empty.
//!
//! It runs the engine on the thread of winit's event loop, which steps it as
//! the first worker of the headless chassis's pool, the others its own
//! threads (ADR-0011). [`Desktop::run`] opens one window as the loop begins,
//! then the GPU on it, waiting for each on that thread, which is the host's to
//! do (ADR-0001 rule 4), and hands both to the program to make its engine
//! with: it opens the window with [`Screen::open_window`] and makes the
//! renderer's config with [`renderer`], as a page does in a browser.
//!
//! Each frame it acquires the window's surface, which with `Fifo` waits for
//! the display to free an image and so paces the frames, says a cycle is due,
//! steps until no actor has anything to run, then presents what the renderer
//! drew and starts mapping what it asked to read back (ADR-0020, ADR-0021). A
//! window hidden or of no size is handed no frame, and the loop waits on its
//! events until it is shown again. What the window hears is pushed onto the
//! window capability's feed as winit hears it, in the window's surface pixels,
//! and brought in as the engine next steps (ADR-0024). Its close button asks
//! the program to stop, as `quit` does, and `Stop` is given the time the host
//! gives it to finish.

#[cfg(not(target_arch = "wasm32"))]
mod gpu;
#[cfg(not(target_arch = "wasm32"))]
mod input;
#[cfg(not(target_arch = "wasm32"))]
mod running;
#[cfg(not(target_arch = "wasm32"))]
mod serving;

#[cfg(not(target_arch = "wasm32"))]
pub use gpu::{Opened, Screen, Unopened, renderer};
#[cfg(not(target_arch = "wasm32"))]
pub use running::{Desktop, Failed, Made};
#[cfg(not(target_arch = "wasm32"))]
pub use serving::Drawing;
