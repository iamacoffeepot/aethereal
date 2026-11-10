//! What paces the frames, as the windows are presented: the display, nothing, or a timer the event loop waits on
//! (ADR-0021); and the cycles run undrawn while the compositor asks for no window's frame (ADR-0045).

#[cfg(test)]
mod tests;

use crate::Presenting;
use crate::redrawing::{SILENT_FRAMES, Silence};
use aethereal_chassis_headless::Timer;

/// What paces the frames, fixed as the program starts.
#[derive(Debug, Clone, Copy)]
pub enum Pacing {
    /// Each frame is asked for as the last is presented: the display paces them, its acquire waiting for an image to
    /// free when `fifo`, and nothing does when `uncapped`.
    Following,
    /// Frames are due on the timer's grid, the event loop waiting until each: one is drawn at each point of it at
    /// most, and one missed while the program was busy is skipped, not queued, as the headless host's cycles are.
    Timed(Timer),
}

/// When the next frame is due, between the loop's events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Due {
    /// When it is asked for: it was as the last was presented, or a window is shown again.
    Asked,
    /// Now: the loop asks for it.
    Now,
    /// At this time on the host's clock, in nanoseconds, which the loop waits until.
    At(u64),
}

impl Pacing {
    /// The pacing of frames presented as `presenting` says, a capped one's grid starting at `start`, in nanoseconds on
    /// the host's clock.
    pub fn new(presenting: Presenting, start: u64) -> Self {
        match presenting {
            Presenting::Fifo | Presenting::Uncapped => Self::Following,
            Presenting::Capped { frames_per_second } => {
                Self::Timed(Timer::new(start, frames_per_second.period_nanos()))
            }
        }
    }

    /// Whether the frame asked for is drawn at `now`: each is when frames follow, and a timed one only when a point of
    /// the grid has come since the last was drawn, so a frame asked for between them, as a resize asks, waits for the
    /// next.
    pub fn draws(&mut self, now: u64) -> bool {
        match self {
            Self::Following => true,
            Self::Timed(timer) => timer.fires(now),
        }
    }

    /// Whether the next frame is asked for as soon as one is presented.
    pub const fn follows(&self) -> bool {
        matches!(self, Self::Following)
    }

    /// When the next frame is due, at `now`.
    pub const fn due(&self, now: u64) -> Due {
        match self {
            Self::Following => Due::Asked,
            Self::Timed(timer) if now >= timer.due() => Due::Now,
            Self::Timed(timer) => Due::At(timer.due()),
        }
    }
}

/// The cycles run undrawn while every window that shows awaits the compositor's ask for its next frame, as a covered
/// window does on Wayland: once they have all waited [`SILENT_FRAMES`] of the budget, a cycle is due on a grid of the
/// budget from then, as the headless host's are, so the program's clock and its actors run on while nothing is drawn,
/// each cycle's delta a frame's, and the first frame drawn once the compositor asks again follows the last of them.
#[derive(Debug, Clone, Copy)]
pub enum Undrawn {
    /// None is planned: a window was drawn or due since the windows last fell silent.
    Unplanned,
    /// They are due on this grid, planned as the windows fell silent.
    Planned(Timer),
}

/// What the loop does about the undrawn cycles at a moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Undrawing {
    /// Nothing: a window is due, or none shows, and the loop waits on the frame asked for or the windows' events.
    Heard,
    /// Runs one now, and looks again at this time on the host's clock, in nanoseconds, when the next is due.
    CycleNow { next_nanos: u64 },
    /// Waits until this time on the host's clock, in nanoseconds, when one is due, unless a window is redrawn first.
    WaitUntil { nanos: u64 },
}

impl Undrawn {
    /// What is due at `now`, the windows that show being as `silence` says, and a frame `budget_nanos` long: the grid,
    /// planned as they fell silent, kept while they stay so, and let go once one is heard.
    pub fn at(&mut self, silence: Silence, budget_nanos: u64, now: u64) -> Undrawing {
        let Silence::Since { nanos } = silence else {
            *self = Self::Unplanned;
            return Undrawing::Heard;
        };
        let mut grid = match *self {
            Self::Planned(grid) => grid,
            Self::Unplanned => {
                let start = nanos.saturating_add(budget_nanos.saturating_mul(SILENT_FRAMES));
                Timer::new(start, budget_nanos)
            }
        };
        let due = grid.fires(now);
        *self = Self::Planned(grid);

        if due {
            Undrawing::CycleNow { next_nanos: grid.due() }
        } else {
            Undrawing::WaitUntil { nanos: grid.due() }
        }
    }

    /// Takes in that a frame was drawn: the next silence plans its grid afresh.
    pub const fn drawn(&mut self) {
        *self = Self::Unplanned;
    }
}
