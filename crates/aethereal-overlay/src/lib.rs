//! The dev overlay (ADR-0053): one root, [`Overlay`], that any program lists, and that knows nothing of any game. The
//! backtick steps it through three stops, [`Stop`]: hidden; a band across the top of the window; and the band with the
//! console beneath it.
//!
//! It is a screen of the widget library (ADR-0056): its view is markup, laid out and drawn by the library in a draw list
//! of its own in the window's interface pass, which follows every other pass of the window's main frame; the program's
//! root list names it last, so it draws over everything the program drew (ADR-0031, ADR-0055). What no stock widget
//! draws is the overlay's own primitive, laid out by the library and drawing into its box: the frame chart, the GPU's
//! graph, the lines' rows, the memory meters' graphs and bars, text in pieces of their own colours, and the band's
//! panel and lines, which the pointer passes through.
//!
//! The band's left column holds the frame chart: sixty seconds of each cycle's time, newest at the right, each column
//! three opaque bars to the slowest, the median, and the fastest of its cycles, sand, lichen, and slate-teal, back to
//! front, on a scale logarithmic in a cycle's time from a quarter of the budget the renderer's frame statistics bring to
//! thirty-two of it, the budget a bright dashed line, with the median, p90, p99, and the longest beneath it, and an
//! ochre tick at each mark. Beneath it on the same axis, each column under the frame chart's of the same span, the GPU's
//! graph stacks each frame's passes by label, each at its median over the column's frames, slate blue, muted violet,
//! and mauve in the order they were first heard, with each pass's median and how busy the GPU was beneath it; and three
//! rows of thin stone bars, each column the most of its frames, draw the draws, triangles, and bytes uploaded, each
//! row's label and newest value in a gutter at its right. Beside the frame chart, its gutter counts the warnings and
//! errors logged since the console was last opened. The band's right column meters the engine's memory, one row each
//! for the process, the blobs, the backlog, the guests, and the GPU, with a bar against its limit for the blobs and the
//! backlog, lichen, then rust from 75% and brick from 90%. Faint lines part the band's two columns, and the left
//! column's three sections.
//!
//! The console's line is the library's text input, which has the screen's focus while the console is open, so the
//! console takes every key of the window; its panel blocks the pointer, and its scrollback is the library's scroll,
//! which the wheel slides, which keeps to its newest row, and whose bar at its right edge shows where the view stands.
//! Its line calls the engine's tools, held by the tools root every engine lists (ADR-0053: Commands are tools), and
//! prints their answers as they would be typed, with the engine's recent log beneath them as it comes; what it has
//! called and not yet heard from shows at the prompt's right end. The overlay offers two tools of its own, `mark` and
//! `memory`, which an agent may call too.

mod binned;
mod chart;
mod colour;
mod console;
mod frames;
mod gpu;
mod layout;
mod lines;
mod marks;
mod meters;
mod overlay;
mod stop;
mod tools;
mod widgets;

pub use overlay::{Overlay, OverlayConfig};
pub use stop::Stop;
