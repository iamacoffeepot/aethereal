//! The dev overlay (ADR-0053): one root, [`Overlay`], that any program lists, and that knows nothing of any game. The
//! backtick steps it through three stops, [`Stop`]: hidden; a band across the top of the window; and the band with the
//! console beneath it.
//!
//! It draws with the renderer's meshes and texts (ADR-0021, ADR-0052), in a draw list of its own in the window's main
//! pass, which is drawn after every placement joined loose, so over what the program drew there (ADR-0031). The band's
//! left column holds the frame chart: sixty seconds of each cycle's time, newest at the right, against the budget the
//! renderer's frame statistics bring as a dashed line, with the median, p90, p99, and the longest beneath it, and an
//! ochre tick at each mark. The band's right end counts the warnings and errors logged since the console was last
//! opened.
//!
//! The console holds the window's key focus while it is open. Its line calls the engine's tools, held by the tools
//! root every engine lists (ADR-0053: Commands are tools), and prints their answers as they would be typed, with the
//! engine's recent log beneath them as it comes. The overlay offers two tools of its own, `mark` and `memory`, which an
//! agent may call too. The band's right column and the GPU's graphs under the chart are pieces of their own, each
//! with what it reads (ADR-0053).

mod chart;
mod colour;
mod console;
mod frames;
mod layout;
mod marks;
mod overlay;
mod stop;
mod tools;

pub use overlay::{Overlay, OverlayConfig};
pub use stop::Stop;
