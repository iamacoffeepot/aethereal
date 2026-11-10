//! The frame budget in a browser (ADR-0053: Frame statistics): a page is not told its display's refresh, so it is
//! measured from the animation frames' own intervals, which the page paces to the display, and said to the renderer
//! through its front as it changes.

#[cfg(test)]
mod tests;

use aethereal_render::stats::{FrameBudget, refresh_nanos};
use std::collections::VecDeque;
use std::mem;

/// How many of the latest intervals the refresh is measured over: about two seconds at 60 Hz.
const KEPT: usize = 120;

/// The shortest interval read as two frames of the display: less is two animation frames in one refresh, as a page
/// may run when it is shown again.
const SHORTEST_NANOS: u64 = 1_000_000;

/// The longest interval read as two frames of the display: more is a page hidden, or a frame that took that long.
const LONGEST_NANOS: u64 = 1_000_000_000;

/// How far the measured refresh moves from the budget last said before it is said again, in parts of a thousand: a
/// little more than the jitter between two of the display's frames.
const MOVED_PER_THOUSAND: u64 = 20;

/// The animation frames' latest intervals, and the budget last said of them. The refresh is the shortest of them, as a
/// frame that took longer than one refresh is shown a refresh or more late, and never early. A display's frames are
/// paced to it, so the budget is a limit; until one is measured, it is the fallback refresh.
pub struct Refresh {
    last: Last,
    intervals: VecDeque<u64>,
    said: FrameBudget,
}

/// The time of the last animation frame, or none before the first.
enum Last {
    Unframed,
    Framed { nanos: u64 },
}

impl Refresh {
    /// No frame measured, aiming at the fallback refresh, which the chassis hands the renderer's front as it is made.
    pub fn new() -> Self {
        Self { last: Last::Unframed, intervals: VecDeque::with_capacity(KEPT), said: Self::untold() }
    }

    /// The budget before one is measured: the fallback refresh, a limit, as the page paces its frames to the display.
    pub const fn untold() -> FrameBudget {
        FrameBudget::Limit { nanos: refresh_nanos(0) }
    }

    /// Takes in an animation frame at `nanos`: answers the budget to say when the shortest interval kept has moved
    /// far enough from the one last said, and none when it has not.
    pub fn frame(&mut self, nanos: u64) -> Option<FrameBudget> {
        let last = mem::replace(&mut self.last, Last::Framed { nanos });
        let Last::Framed { nanos: before } = last else {
            return None;
        };
        let interval = nanos.saturating_sub(before);
        let displayed = (SHORTEST_NANOS..=LONGEST_NANOS).contains(&interval);
        if !displayed {
            return None;
        }
        if self.intervals.len() == KEPT {
            self.intervals.pop_front();
        }
        self.intervals.push_back(interval);

        let shortest = self.intervals.iter().copied().min()?;
        let said = self.said.nanos();
        let moved = shortest.abs_diff(said).saturating_mul(1000) > said.saturating_mul(MOVED_PER_THOUSAND);
        if !moved {
            return None;
        }
        self.said = FrameBudget::Limit { nanos: shortest };

        Some(self.said)
    }
}
