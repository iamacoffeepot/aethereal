//! Whether the compositor has asked for a window's next frame (ADR-0021, ADR-0045). A window presented `fifo` is
//! presented again only once the compositor asks: Mesa's Wayland driver waits, as it presents a `fifo` frame, for the
//! compositor's frame callback on the frame before, and a compositor sends a covered window none, so a present issued
//! before the compositor asked can wait on the event loop's thread until the window is uncovered, with every actor
//! that thread steps. The chassis tells winit just before each `fifo` present, so winit holds the window's next redraw
//! until that callback, and draws the window's next frame as that redraw comes.

#[cfg(test)]
mod tests;

use crate::Presenting;
use crate::serving::Visible;

/// Where a window's frames stand against the compositor's asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Redraw {
    /// Its redraw has come since its last frame was presented, none has been presented yet, or its frames are
    /// presented without waiting on the compositor: its next frame is drawn and presented at once.
    Due,
    /// Its last frame was presented `fifo`, and its redraw has not come since: it is drawn no further until it does.
    Awaiting,
}

impl Redraw {
    /// What follows a frame presented as `presenting` says: a `fifo` one awaits the compositor's ask, and one presented
    /// without waiting is due again at once.
    pub const fn presented(presenting: Presenting) -> Self {
        match presenting {
            Presenting::Fifo => Self::Awaiting,
            Presenting::Uncapped | Presenting::Capped { .. } => Self::Due,
        }
    }
}

/// Whether a window is drawn this frame: shown, at a size, and its redraw due.
pub fn drawn(visible: Visible, sized: bool, redraw: Redraw) -> bool {
    let shows = visible == Visible::Shown;
    let due = redraw == Redraw::Due;

    shows && sized && due
}
