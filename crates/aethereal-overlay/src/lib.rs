//! The dev overlay (ADR-0053): one root, [`Overlay`], that any program lists, and that knows nothing of any game. The
//! backtick steps it through three stops, [`Stop`]: hidden; a band across the top of the window; and the band with the
//! console beneath it.
//!
//! It draws with the renderer's meshes and texts (ADR-0021, ADR-0052), in a draw list of its own in the window's main
//! pass, which is drawn after every placement joined loose, so over what the program drew there (ADR-0031). The band's
//! left column holds the frame chart: sixty seconds of each cycle's time, newest at the right, each column three opaque
//! bars to the slowest, the median, and the fastest of its cycles, sand, lichen, and slate-teal, back to front, on a
//! scale logarithmic in a cycle's time from a quarter of the budget the renderer's frame statistics bring to thirty-two
//! of it, the budget a bright dashed line, with the median, p90, p99, and the longest beneath it, and an ochre tick at
//! each mark. Beneath it on the same axis, each column under the frame chart's of the same span, the GPU's graph
//! stacks each frame's passes by label, each at its median over the column's frames, slate blue, muted violet, and
//! mauve in the order they were first heard, on a linear scale to twice the budget, with each pass's median and how
//! busy the GPU was beneath it; and four rows of thin stone bars, each column the most of its frames, draw the draws,
//! triangles, bytes uploaded, and GPU memory, each row's label and newest value in a gutter at its right. The band's
//! right end counts the warnings and errors logged since the console was last opened.
//!
//! The console holds the window's key focus while it is open, and with it the wheel, which scrolls its scrollback; a
//! scroll bar at its right edge shows where the view stands. Its line calls the engine's tools, held by the tools
//! root every engine lists (ADR-0053: Commands are tools), and prints their answers as they would be typed, with the
//! engine's recent log beneath them as it comes; what it has called and not yet heard from shows at the prompt's right
//! end. The band, the scrollback, and the prompt's strip are each a tint of slate of their own, parted by faint lines.
//! The overlay offers two tools of its own, `mark` and `memory`, which an agent may call too. The band's right column
//! is a piece of its own, with what it reads (ADR-0053).

mod binned;
mod chart;
mod colour;
mod console;
mod frames;
mod gpu;
mod layout;
mod lines;
mod marks;
mod overlay;
mod scrollbar;
mod stop;
mod tools;

pub use overlay::{Overlay, OverlayConfig};
pub use stop::Stop;
