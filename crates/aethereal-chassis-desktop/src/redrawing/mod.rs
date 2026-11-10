//! Whether the compositor has asked for a window's next frame (ADR-0021, ADR-0045). A window presented `fifo` is
//! presented again only once the compositor asks: Mesa's Wayland driver waits, as it presents a `fifo` frame, for the
//! compositor's frame callback on the frame before, and a compositor sends a covered window none, so a present issued
//! before the compositor asked can wait on the event loop's thread until the window is uncovered, with every actor
//! that thread steps. The chassis tells winit just before each `fifo` present, so winit holds the window's next redraw
//! until that callback. It draws a window's next frame while it waits, and holds it, and presents it as the redraw
//! comes, so the present lands as soon as the compositor asks, as it did when the driver waited for it.

#[cfg(test)]
mod tests;

use crate::Presenting;
use crate::serving::Visible;

/// Where a window's frames stand against the compositor's asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Redraw {
    /// Its redraw has come since its last frame was presented, none has been presented yet, or its frames are
    /// presented without waiting on the compositor: the next frame drawn is presented at once.
    Due,
    /// Its last frame was presented `fifo`, and its redraw has not come since: the next frame drawn is held until it
    /// does.
    Awaiting,
    /// As `Awaiting`, and its next frame is drawn and held, presented as the redraw comes: it is drawn no further.
    Drawn,
}

/// What becomes of a frame the renderer drew of a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drew {
    /// It is presented at once.
    Present,
    /// It is held, and presented as the compositor asks for it.
    Hold,
}

/// What a window's redraw does, as the compositor asks for its next frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asked {
    /// Presents the frame held, the window then awaiting the next ask.
    PresentHeld,
    /// Makes the window due: its next frame is drawn and presented at once.
    Due,
}

impl Redraw {
    /// What follows a frame presented at once as `presenting` says: a `fifo` one awaits the compositor's ask, and one
    /// presented without waiting is due again at once.
    pub const fn presented(presenting: Presenting) -> Self {
        match presenting {
            Presenting::Fifo => Self::Awaiting,
            Presenting::Uncapped | Presenting::Capped { .. } => Self::Due,
        }
    }

    /// What becomes of the frame the renderer drew of the window: held while it awaits the compositor's ask.
    pub const fn drew(self) -> Drew {
        match self {
            Self::Awaiting | Self::Drawn => Drew::Hold,
            Self::Due => Drew::Present,
        }
    }

    /// The window, its frame held: drawn, ahead of the ask it awaited.
    pub const fn held(self) -> Self {
        match self {
            Self::Awaiting | Self::Drawn => Self::Drawn,
            Self::Due => Self::Due,
        }
    }

    /// What the compositor's ask for the window's next frame does.
    pub const fn asked(self) -> Asked {
        match self {
            Self::Drawn => Asked::PresentHeld,
            Self::Due | Self::Awaiting => Asked::Due,
        }
    }
}

/// Whether a window is drawn this frame: shown, at a size, and no frame of it held.
pub fn drawn(visible: Visible, sized: bool, redraw: Redraw) -> bool {
    let shows = visible == Visible::Shown;
    let unheld = redraw != Redraw::Drawn;

    shows && sized && unheld
}
