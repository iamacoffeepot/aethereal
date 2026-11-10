//! Samples binned into the frame chart's columns (ADR-0053: The three stops): what the renderer tells of each frame,
//! the GPU's time over its passes and what it drew, kept over the last [`KEPT_NANOS`] of the program's time and cut
//! into the same spans the frame chart's columns cover, so a column of the GPU's graph, or of a line beneath it, stands
//! straight under the frame chart's column of the same frames.
//!
//! As the frame chart's columns do (`frames`), a column covers a fixed span, the `n`th from `n` spans to `n + 1`, and
//! what it holds is worked out once, as its span passes; a chart cut into another count is binned again from the
//! samples kept. What a column comes to is the bin's to say ([`Accrue`]): a stack of the passes' medians, or the most
//! of each line. A bin that keeps a sample for a median keeps a bounded one (`thinned`).

#[cfg(test)]
mod tests;
mod thinned;

#[cfg(test)]
pub use thinned::MOST_KEPT;
pub use thinned::Thinned;

use crate::frames::{KEPT_NANOS, WINDOW_NANOS, span_nanos};
use aethereal_data::Plain;
use core::fmt::Debug;
use std::collections::VecDeque;

/// What one column makes of the samples that fall in its span.
pub trait Accrue: Clone + Debug + Eq {
    /// One frame's sample.
    type Sample: Copy + Debug + Eq;
    /// What a column's samples come to.
    type Column: Copy + Debug + Eq;

    /// A column whose first sample is `sample`.
    fn first(sample: Self::Sample) -> Self;

    /// Takes in another sample.
    fn take(&mut self, sample: Self::Sample);

    /// What the samples so far come to.
    fn column(&self) -> Self::Column;
}

/// One frame's sample, beside the program's time of the cycle it is of.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamped<S> {
    pub elapsed_nanos: u64,
    pub sample: S,
}

/// What one column came to, beside the span of the program's time it covers, counted from its start.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spanned<C> {
    pub span: u64,
    pub column: C,
}

/// The newest column, while a sample has fallen in one.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
enum Open<A> {
    /// No sample has been taken in since the columns were cut.
    Nothing,
    /// The samples of the newest column, of `span`, so far.
    Filling { span: u64, bin: A },
}

/// The samples over the last [`KEPT_NANOS`], oldest first, and the columns they fall in at the span the chart was last
/// cut at: those closed, oldest first, and the open one.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub struct Binned<A: Accrue> {
    span_nanos: u64,
    samples: VecDeque<Stamped<A::Sample>>,
    closed: VecDeque<Spanned<A::Column>>,
    open: Open<A>,
}

impl<A: Accrue> Default for Binned<A> {
    fn default() -> Self {
        Self { span_nanos: span_nanos(1), samples: VecDeque::new(), closed: VecDeque::new(), open: Open::Nothing }
    }
}

impl<A: Accrue> Binned<A> {
    /// Takes in the sample of the cycle at `elapsed_nanos`, and lets go of every sample more than [`KEPT_NANOS`] older
    /// than it.
    pub fn push(&mut self, elapsed_nanos: u64, sample: A::Sample) {
        self.samples.push_back(Stamped { elapsed_nanos, sample });
        self.bin(elapsed_nanos, sample);
        while let Some(oldest) = self.samples.front() {
            let kept = elapsed_nanos.saturating_sub(oldest.elapsed_nanos) < KEPT_NANOS;
            if kept {
                break;
            }
            self.samples.pop_front();
        }
    }

    /// Takes `sample` into the open column when it falls in its span, or one before it, as a frame told late is
    /// still of the column it was drawn in; or else into a new one, the open column closed as what it came to.
    fn bin(&mut self, elapsed_nanos: u64, sample: A::Sample) {
        let span = elapsed_nanos / self.span_nanos.max(1);
        if let Open::Filling { span: open, bin } = &mut self.open {
            let within = span <= *open;
            if within {
                bin.take(sample);
                return;
            }
            self.closed.push_back(Spanned { span: *open, column: bin.column() });
        }
        self.open = Open::Filling { span, bin: A::first(sample) };
    }

    /// The newest sample: none while none has been taken in.
    pub fn newest(&self) -> Option<A::Sample> {
        self.samples.back().map(|stamped| stamped.sample)
    }

    /// The program's time of the newest sample: zero while none has been taken in.
    fn newest_nanos(&self) -> u64 {
        self.samples.back().map_or(0, |stamped| stamped.elapsed_nanos)
    }

    /// Cuts the columns as a chart of `count` is: binned again from the samples kept when it was last cut into
    /// another count, and those from the newest back to the one `count + 1` spans before it kept, as the chart's width
    /// holds a little more than `count` columns and the oldest two slide off its left edge.
    pub fn cut(&mut self, count: u32) {
        let span_nanos = span_nanos(count);
        let recut = self.span_nanos != span_nanos;
        if recut {
            self.span_nanos = span_nanos;
            self.closed.clear();
            self.open = Open::Nothing;
            for index in 0..self.samples.len() {
                let stamped = self.samples[index];
                self.bin(stamped.elapsed_nanos, stamped.sample);
            }
        }
        let oldest = (self.newest_nanos() / span_nanos).saturating_sub(u64::from(count) + 1);
        while let Some(column) = self.closed.front() {
            let kept = column.span >= oldest;
            if kept {
                break;
            }
            self.closed.pop_front();
        }
    }

    /// The columns as they were last cut, from the newest, the open one, back to the oldest kept.
    pub fn newest_first(&self) -> impl Iterator<Item = Spanned<A::Column>> + Clone + '_ {
        let open = match &self.open {
            Open::Nothing => None,
            Open::Filling { span, bin } => Some(Spanned { span: *span, column: bin.column() }),
        };

        open.into_iter().chain(self.closed.iter().rev().copied())
    }

    /// The samples of the last [`WINDOW_NANOS`] before the newest, from the newest back.
    pub fn window(&self) -> impl Iterator<Item = A::Sample> + '_ {
        let newest = self.newest_nanos();

        self.samples
            .iter()
            .rev()
            .take_while(move |stamped| newest.saturating_sub(stamped.elapsed_nanos) < WINDOW_NANOS)
            .map(|stamped| stamped.sample)
    }
}

/// The median of `values` by nearest rank: zero when there are none.
pub fn median(mut values: Vec<u64>) -> u64 {
    values.sort_unstable();
    let at = values.len().div_ceil(2).saturating_sub(1);

    values.get(at).copied().unwrap_or_default()
}
