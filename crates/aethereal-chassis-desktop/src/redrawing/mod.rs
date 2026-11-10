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

/// How many of the budget's frames every window that shows may wait on the compositor before the program runs on
/// undrawn: a tenth of a second at 60 Hz, past a frame or two late, as a slow frame comes.
pub const SILENT_FRAMES: u64 = 6;

/// Where a window's frames stand against the compositor's asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Redraw {
    /// Its redraw has come since its last frame was presented, none has been presented yet, or its frames are
    /// presented without waiting on the compositor: the next frame drawn is presented at once.
    Due,
    /// Its last frame was presented `fifo` at this time on the chassis's clock, in nanoseconds, and its redraw has
    /// not come since: the next frame drawn is held until it does.
    Awaiting { presented_nanos: u64 },
    /// As `Awaiting`, and its next frame is drawn and held, presented as the redraw comes: it is drawn no further.
    Drawn { presented_nanos: u64 },
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
    /// What follows a frame presented at once as `presenting` says, at `now`: a `fifo` one awaits the compositor's
    /// ask, and one presented without waiting is due again at once.
    pub const fn presented(presenting: Presenting, now: u64) -> Self {
        match presenting {
            Presenting::Fifo => Self::Awaiting { presented_nanos: now },
            Presenting::Uncapped | Presenting::Capped { .. } => Self::Due,
        }
    }

    /// What becomes of the frame the renderer drew of the window: held while it awaits the compositor's ask.
    pub const fn drew(self) -> Drew {
        match self {
            Self::Awaiting { .. } | Self::Drawn { .. } => Drew::Hold,
            Self::Due => Drew::Present,
        }
    }

    /// The window, its frame held: drawn, ahead of the ask it awaited.
    pub const fn held(self) -> Self {
        match self {
            Self::Awaiting { presented_nanos } | Self::Drawn { presented_nanos } => Self::Drawn { presented_nanos },
            Self::Due => Self::Due,
        }
    }

    /// What the compositor's ask for the window's next frame does.
    pub const fn asked(self) -> Asked {
        match self {
            Self::Drawn { .. } => Asked::PresentHeld,
            Self::Due | Self::Awaiting { .. } => Asked::Due,
        }
    }

    /// When the window's last frame was presented, while it awaits the compositor's ask: none while it is due.
    const fn awaiting_since(self) -> Silence {
        match self {
            Self::Due => Silence::Heard,
            Self::Awaiting { presented_nanos } | Self::Drawn { presented_nanos } => {
                Silence::Since { nanos: presented_nanos }
            }
        }
    }
}

/// Whether a window is drawn this frame: shown, at a size, and no frame of it held.
pub fn drawn(visible: Visible, sized: bool, redraw: Redraw) -> bool {
    let shows = visible == Visible::Shown;
    let unheld = !matches!(redraw, Redraw::Drawn { .. });

    shows && sized && unheld
}

/// When a held cycle owed runs, as the windows that show await the compositor (ADR-0045).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    /// Now: no window holds a frame, or each that does has waited on the compositor for [`SILENT_FRAMES`], as a
    /// covered window does, and is left undrawn.
    RunNow,
    /// Once a window holding a frame, presented lately, is asked for its next and presents it, or at this time on the
    /// chassis's clock, in nanoseconds, when the last of them has waited [`SILENT_FRAMES`].
    WaitUntil { nanos: u64 },
}

impl Hold {
    /// When a held cycle owed at `now` runs, `windows` the redraw of each window that shows, and a frame of the
    /// budget `budget_nanos` long.
    pub fn of(windows: impl IntoIterator<Item = Redraw>, budget_nanos: u64, now: u64) -> Self {
        let silent_after = budget_nanos.saturating_mul(SILENT_FRAMES);
        let mut hold = Self::RunNow;
        for redraw in windows {
            let Redraw::Drawn { presented_nanos } = redraw else {
                continue;
            };
            let silent_at = presented_nanos.saturating_add(silent_after);
            if silent_at <= now {
                continue;
            }
            let until = match hold {
                Self::RunNow => silent_at,
                Self::WaitUntil { nanos } => nanos.max(silent_at),
            };
            hold = Self::WaitUntil { nanos: until };
        }

        hold
    }
}

/// How long the windows that show have waited on the compositor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Silence {
    /// One is due, or none shows: frames come as they are asked for.
    Heard,
    /// Every window that shows awaits its redraw, the last of them presented at this time on the chassis's clock, in
    /// nanoseconds.
    Since { nanos: u64 },
}

impl Silence {
    /// The silence of `windows`, each the redraw of a window that shows.
    pub fn of(windows: impl IntoIterator<Item = Redraw>) -> Self {
        let mut silence = Self::Heard;
        for redraw in windows {
            let Self::Since { nanos: presented_nanos } = redraw.awaiting_since() else {
                return Self::Heard;
            };
            let since = match silence {
                Self::Heard => presented_nanos,
                Self::Since { nanos } => nanos.max(presented_nanos),
            };
            silence = Self::Since { nanos: since };
        }

        silence
    }
}
