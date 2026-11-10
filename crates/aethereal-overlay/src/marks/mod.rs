//! Marks (ADR-0053: The overlay's own commands): moments pinned by the `mark` tool, by a person at the console or an
//! agent, each drawn as an ochre tick on the frame chart at its time, sliding left with the columns until it is older
//! than the chart's sixty seconds. A mark is told to the overlay between cycles, so it waits for the next `Tick` to
//! be stamped with the program's time: the overlay hears no `Tick` while it is hidden, and its newest cycle may be long
//! past.

#[cfg(test)]
mod tests;

use crate::frames::WINDOW_NANOS;
use aethereal_data::Plain;
use core::mem;
use std::collections::VecDeque;

/// The most marks kept: more than a person pins in a minute, and a bound on what an agent's marks hold.
pub const MOST_MARKS: u32 = 64;

/// The program's time of each mark pinned in the last sixty seconds, oldest first, and the text of each mark told
/// since the last `Tick`, waiting to be stamped.
#[derive(Plain, Debug, Clone, Default, PartialEq, Eq)]
pub struct Marks {
    pinned: VecDeque<u64>,
    waiting: Vec<String>,
}

impl Marks {
    /// Takes in a mark saying `text`, to be pinned at the next `Tick`; past [`MOST_MARKS`] waiting, the newest is
    /// dropped.
    pub fn wait(&mut self, text: String) {
        let room = self.waiting.len() < MOST_MARKS as usize;
        if room {
            self.waiting.push(text);
        }
    }

    /// Pins every mark waiting at `at_nanos`, the time of the `Tick` heard, and answers their texts, in the order
    /// they were told.
    pub fn stamp(&mut self, at_nanos: u64) -> Vec<String> {
        let stamped = mem::take(&mut self.waiting);
        for _ in &stamped {
            self.pin(at_nanos);
        }

        stamped
    }

    /// Pins a mark at `at_nanos` of the program's time, the oldest leaving past [`MOST_MARKS`].
    pub fn pin(&mut self, at_nanos: u64) {
        self.pinned.push_back(at_nanos);
        while self.pinned.len() > MOST_MARKS as usize {
            self.pinned.pop_front();
        }
    }

    /// Lets go of every mark older than the chart reaches back from `newest_nanos`.
    pub fn keep_since(&mut self, newest_nanos: u64) {
        while let Some(&oldest) = self.pinned.front() {
            let shown = newest_nanos.saturating_sub(oldest) < WINDOW_NANOS;
            if shown {
                break;
            }
            self.pinned.pop_front();
        }
    }

    /// The program's time of each mark kept, oldest first.
    pub fn times(&self) -> impl Iterator<Item = u64> + '_ {
        self.pinned.iter().copied()
    }
}
