//! What paces the frames, as the windows are presented: the display, nothing, or a timer the event loop waits on
//! (ADR-0021).

#[cfg(test)]
mod tests;

use crate::Presenting;
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
