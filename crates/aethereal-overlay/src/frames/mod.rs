//! The frame times the chart is drawn from (ADR-0053: The three stops): each cycle's `Tick.delta_nanos` over the last
//! sixty seconds of the program's time (ADR-0020), read as the chart's columns and as the figures beneath it.

#[cfg(test)]
mod tests;

use crate::colour::{BONE, STONE};
use aethereal_actor::Tick;
use aethereal_data::Plain;
use aethereal_render::text::Run;
use std::collections::VecDeque;

/// How far back the chart reaches: sixty seconds of the program's time.
pub const WINDOW_NANOS: u64 = 60_000_000_000;

/// What a figure is rounded to, as it is shown: a tenth of a millisecond.
const SHOWN_NANOS: u64 = 100_000;

/// One cycle's time: when its `Tick` came, in the program's time, and how long since the cycle before.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
struct Sample {
    elapsed_nanos: u64,
    delta_nanos: u64,
}

/// The cycles' times over the last [`WINDOW_NANOS`], oldest first: each `Tick` heard is pushed at the back, and those
/// more than the window older than it leave at the front.
#[derive(Plain, Debug, Default, Clone, PartialEq, Eq)]
pub struct FrameTimes {
    samples: VecDeque<Sample>,
}

/// One column of the chart that a cycle fell in: its place counted from the left, and the longest cycle in it, so a
/// spike reads however many cycles a column holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Column {
    pub at: u32,
    pub longest_nanos: u64,
}

/// The figures beneath the chart over the window, each in nanoseconds, rounded to a tenth of a millisecond as it is
/// shown, so two that show the same are equal: the median, the 90th and 99th percentiles, and the longest.
#[derive(Plain, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Figures {
    median: u64,
    p90: u64,
    p99: u64,
    longest: u64,
}

impl FrameTimes {
    /// Takes in the cycle `tick` begins, and lets go of every cycle more than the window older than it.
    pub fn push(&mut self, tick: &Tick) {
        self.samples.push_back(Sample { elapsed_nanos: tick.elapsed_nanos, delta_nanos: tick.delta_nanos });
        let newest = tick.elapsed_nanos;
        while let Some(oldest) = self.samples.front() {
            let fresh = newest.saturating_sub(oldest.elapsed_nanos) < WINDOW_NANOS;
            if fresh {
                break;
            }
            self.samples.pop_front();
        }
    }

    /// The window cut into `count` columns of equal time, the newest at the right, as each cycle fell in them: only
    /// the columns some cycle fell in, from the right. Cycles are kept oldest first, so each column's are next to
    /// each other, and one pass back from the newest finds them.
    pub fn columns(&self, count: u32) -> Vec<Column> {
        let Some(newest) = self.samples.back().map(|sample| sample.elapsed_nanos) else {
            return Vec::new();
        };
        if count == 0 {
            return Vec::new();
        }
        let span = WINDOW_NANOS.div_ceil(u64::from(count));
        let mut columns: Vec<Column> = Vec::new();
        for sample in self.samples.iter().rev() {
            // A cycle in no column is older than the window, and so is every one before it.
            let Ok(back) = u32::try_from((newest - sample.elapsed_nanos) / span) else {
                break;
            };
            let Some(at) = (count - 1).checked_sub(back) else {
                break;
            };
            match columns.last_mut() {
                Some(column) if column.at == at => column.longest_nanos = column.longest_nanos.max(sample.delta_nanos),
                _ => columns.push(Column { at, longest_nanos: sample.delta_nanos }),
            }
        }

        columns
    }

    /// The figures over the window, by nearest rank: zero for each while no cycle has been heard.
    pub fn figures(&self) -> Figures {
        let mut sorted: Vec<u64> = self.samples.iter().map(|sample| sample.delta_nanos).collect();
        sorted.sort_unstable();
        let rank = |percent: usize| {
            let at = (sorted.len() * percent).div_ceil(100).saturating_sub(1);
            sorted.get(at).copied().map_or(0, shown)
        };

        Figures { median: rank(50), p90: rank(90), p99: rank(99), longest: rank(100) }
    }
}

/// `nanos` rounded to the nearest tenth of a millisecond.
const fn shown(nanos: u64) -> u64 {
    nanos.saturating_add(SHOWN_NANOS / 2) / SHOWN_NANOS * SHOWN_NANOS
}

/// `nanos`, a whole number of tenths of a millisecond, written in milliseconds with one place: `16.7`.
fn millis(nanos: u64) -> String {
    let tenths = nanos / SHOWN_NANOS;

    format!("{}.{}", tenths / 10, tenths % 10)
}

impl Figures {
    /// The runs of the text that shows them: `med 16.7  p90 16.9  p99 17.4  max 33.1 ms`, each label in stone and
    /// each figure in bone.
    pub fn runs(self) -> Vec<Run> {
        let label = |text: &str| Run::new(text, STONE.opaque());
        let figure = |nanos: u64| Run::new(&millis(nanos), BONE.opaque());

        vec![
            label("med "),
            figure(self.median),
            label("  p90 "),
            figure(self.p90),
            label("  p99 "),
            figure(self.p99),
            label("  max "),
            figure(self.longest),
            label(" ms"),
        ]
    }
}
