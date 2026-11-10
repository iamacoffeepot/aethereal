//! The frame times the chart is drawn from (ADR-0053: The three stops): each cycle's `Tick.delta_nanos` over the last
//! sixty seconds of the program's time (ADR-0020), read as the chart's columns and as the figures beneath it.
//!
//! A column covers a fixed span of the program's time, the `n`th from `n` spans to `n + 1`, so once its span has
//! passed what it holds never changes; the chart slides the columns left as the program's time passes, rather than
//! cutting the cycles again from the newest, which would hop a spike from column to column.

#[cfg(test)]
mod tests;

use crate::colour::{BONE, STONE};
use aethereal_actor::Tick;
use aethereal_data::Plain;
use aethereal_render::text::Run;
use core::iter::{Peekable, Rev};
use std::collections::{VecDeque, vec_deque};

/// How far back the chart reaches: sixty seconds of the program's time.
pub const WINDOW_NANOS: u64 = 60_000_000_000;

/// How far back the cycles are kept: three windows, as the oldest column a chart of a single column draws begins up
/// to two spans of a whole window before the newest, and a column's longest cycle must stay while any of it shows.
const KEPT_NANOS: u64 = 3 * WINDOW_NANOS;

/// What a figure is rounded to, as it is shown: a tenth of a millisecond.
const SHOWN_NANOS: u64 = 100_000;

/// One cycle's time: when its `Tick` came, in the program's time, and how long since the cycle before.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
struct Sample {
    elapsed_nanos: u64,
    delta_nanos: u64,
}

/// The cycles' times over the last [`KEPT_NANOS`], oldest first: each `Tick` heard is pushed at the back, and those
/// more than that older than it leave at the front.
#[derive(Plain, Debug, Default, Clone, PartialEq, Eq)]
pub struct FrameTimes {
    samples: VecDeque<Sample>,
}

/// One column of the chart that a cycle fell in: which span of the program's time it covers, counted from its start,
/// and the longest cycle in it, so a spike reads however many cycles a column holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Column {
    pub span: u64,
    pub longest_nanos: u64,
}

/// Where the columns stand at the newest cycle: the span it fell in, drawn at the chart's right edge, and how far
/// through that span it came, from zero to under one, the share of a column's width every column is moved left by.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slide {
    pub newest: u64,
    pub through: f32,
}

/// The columns some cycle fell in, from the newest back to the oldest the chart draws, each worked out as it is asked
/// for: the cycles are kept oldest first, so each column's are next to each other, and one pass back from the newest
/// finds them, with nothing gathered on the way.
pub struct Columns<'times> {
    samples: Peekable<Rev<vec_deque::Iter<'times, Sample>>>,
    span_nanos: u64,
    oldest: u64,
}

impl Iterator for Columns<'_> {
    type Item = Column;

    fn next(&mut self) -> Option<Column> {
        let first = self.samples.next()?;
        let span = first.elapsed_nanos / self.span_nanos;
        // A cycle older than the oldest column is not drawn, and neither is any before it.
        if span < self.oldest {
            return None;
        }
        let mut longest_nanos = first.delta_nanos;
        while let Some(sample) = self.samples.next_if(|sample| sample.elapsed_nanos / self.span_nanos == span) {
            longest_nanos = longest_nanos.max(sample.delta_nanos);
        }

        Some(Column { span, longest_nanos })
    }
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
    /// Takes in the cycle `tick` begins, and lets go of every cycle more than [`KEPT_NANOS`] older than it.
    pub fn push(&mut self, tick: &Tick) {
        self.samples.push_back(Sample { elapsed_nanos: tick.elapsed_nanos, delta_nanos: tick.delta_nanos });
        let newest = tick.elapsed_nanos;
        while let Some(oldest) = self.samples.front() {
            let kept = newest.saturating_sub(oldest.elapsed_nanos) < KEPT_NANOS;
            if kept {
                break;
            }
            self.samples.pop_front();
        }
    }

    /// The program's time of the newest cycle: zero while none has been heard.
    fn newest_nanos(&self) -> u64 {
        self.samples.back().map_or(0, |sample| sample.elapsed_nanos)
    }

    /// How long a column's span is when the window is cut into `count` of them: the whole window when `count` is
    /// zero. A minute cut into at most a u32's count is never cut to nothing.
    fn span_nanos(count: u32) -> u64 {
        WINDOW_NANOS.div_ceil(u64::from(count.max(1)))
    }

    /// The columns of a chart of `count`, each the span of the program's time it covers and the longest cycle in it:
    /// only those some cycle fell in, from the newest, back to the one `count + 1` spans before it, as the chart's
    /// width holds a little more than `count` columns and the oldest two slide off its left edge.
    pub fn columns(&self, count: u32) -> Columns<'_> {
        let span_nanos = Self::span_nanos(count);
        let oldest = (self.newest_nanos() / span_nanos).saturating_sub(u64::from(count) + 1);

        Columns { samples: self.samples.iter().rev().peekable(), span_nanos, oldest }
    }

    /// Where the columns of a chart of `count` stand at the newest cycle.
    pub fn slide(&self, count: u32) -> Slide {
        let span_nanos = Self::span_nanos(count);
        let newest_nanos = self.newest_nanos();
        // Nanoseconds of a span, at most a minute, which an f32 holds closely enough to draw.
        #[allow(clippy::cast_precision_loss)]
        let through = (newest_nanos % span_nanos) as f32 / span_nanos as f32;

        Slide { newest: newest_nanos / span_nanos, through }
    }

    /// The figures over the window, by nearest rank: zero for each while no cycle has been heard.
    pub fn figures(&self) -> Figures {
        let newest = self.newest_nanos();
        let mut sorted: Vec<u64> = self
            .samples
            .iter()
            .rev()
            .take_while(|sample| newest.saturating_sub(sample.elapsed_nanos) < WINDOW_NANOS)
            .map(|sample| sample.delta_nanos)
            .collect();
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
