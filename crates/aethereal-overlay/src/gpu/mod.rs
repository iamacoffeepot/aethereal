//! The GPU's time over each frame's passes (ADR-0053: The three stops), as the renderer's [`GpuTimes`] tell it a frame
//! or more after the frame: kept over the frame chart's axis by the cycle each frame was drawn in, and cut into its
//! columns, each a stack of its frames' passes, every pass at its median over the column's frames, so the stack is a
//! typical frame of the column, drawn to a top ranged to the tallest stack (`graph`); and the figures beneath the graph, each pass's median over the window and the
//! share of a frame the GPU was busy (`figures`). A pass is known by its label, and given a place in the stack and a
//! colour in the order the labels are first heard (`labels`). On a device without timestamps there is nothing to keep.

mod figures;
mod graph;
mod labels;
#[cfg(test)]
mod tests;

pub use figures::{GpuFigures, unsupported};
pub use graph::{GraphTop, graph};

use crate::binned::{Accrue, Binned, Spanned, Thinned, median};
use aethereal_data::Plain;
use aethereal_render::stats::GpuTimes;
use core::array;
use figures::PassFigure;
use labels::{Labels, Slot};

/// The most passes the graph stacks, each by its own label: the first heard, as a program's passes are few and fixed.
/// A pass heard past them is left out of the stack, and counted in the frame's total still.
pub const MOST_PASSES: u32 = 6;

/// [`MOST_PASSES`], as the length of a stack.
const STACKED: usize = MOST_PASSES as usize;

/// What the overlay knows of the GPU's time: the passes' times as they come, or that the device has none.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub enum GpuTiming {
    /// The device has timestamps: each frame's passes, as they have come.
    Timed(PassTimes),
    /// The device has no timestamps, as the renderer said as the overlay subscribed.
    Unsupported,
}

impl Default for GpuTiming {
    fn default() -> Self {
        Self::Timed(PassTimes::default())
    }
}

impl GpuTiming {
    /// Takes in what the renderer told: a frame's passes kept, or the device's lack of timestamps, after which nothing
    /// is kept.
    pub fn heard(&mut self, told: GpuTimes) {
        match (self, told) {
            (Self::Timed(times), GpuTimes::Took { elapsed_nanos, passes }) => {
                times.take(elapsed_nanos, passes.iter().map(|pass| (pass.label.as_str(), pass.nanos)));
            }
            (timing, GpuTimes::Unsupported) => *timing = Self::Unsupported,
            (Self::Unsupported, GpuTimes::Took { .. }) => {}
        }
    }
}

/// Each pass's nanoseconds in one frame, at its label's place: nothing where the frame ran no pass of that label.
#[derive(Plain, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Stack {
    pub nanos: [u64; STACKED],
}

impl Stack {
    /// The stack's height: its passes' nanoseconds together.
    pub fn total_nanos(self) -> u64 {
        self.nanos.iter().fold(0, |total, nanos| total.saturating_add(*nanos))
    }
}

/// One frame's passes: stacked by label, and their total, which counts a pass left out of the stack too.
#[derive(Plain, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Timed {
    stack: Stack,
    total_nanos: u64,
}

/// One column's frames so far, an even sample of them, for each pass's median.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub struct PassBin {
    sample: Thinned<Stack>,
}

impl Accrue for PassBin {
    type Sample = Timed;
    type Column = Stack;

    fn first(timed: Timed) -> Self {
        Self { sample: Thinned::first(timed.stack) }
    }

    fn take(&mut self, timed: Timed) {
        self.sample.take(timed.stack);
    }

    /// Each pass at its median over the frames kept, each pass's median taken on its own.
    fn column(&self) -> Stack {
        let kept = self.sample.kept();
        let nanos = array::from_fn(|index| median(kept.iter().map(|stack| stack.nanos[index]).collect()));

        Stack { nanos }
    }
}

/// The passes' labels in the order they were first heard, and each frame's passes over the frame chart's axis.
#[derive(Plain, Debug, Default, Clone, PartialEq, Eq)]
pub struct PassTimes {
    labels: Labels,
    binned: Binned<PassBin>,
}

impl PassTimes {
    /// Takes in the passes of the frame of the cycle at `elapsed_nanos`, each its label and nanoseconds: each into its
    /// label's place, two of one label together, as two windows' frames each run a pass of it; and all of them into
    /// the frame's total.
    pub fn take<'a>(&mut self, elapsed_nanos: u64, passes: impl IntoIterator<Item = (&'a str, u64)>) {
        let mut timed = Timed::default();
        for (label, nanos) in passes {
            timed.total_nanos = timed.total_nanos.saturating_add(nanos);
            match self.labels.slot(label) {
                Slot::Pass(index) => timed.stack.nanos[index] = timed.stack.nanos[index].saturating_add(nanos),
                Slot::Past => {}
            }
        }
        self.binned.push(elapsed_nanos, timed);
    }

    /// The graph's columns for a chart of `count`, from the newest back, cut as the frame chart's are.
    pub fn columns(&mut self, count: u32) -> impl Iterator<Item = Spanned<Stack>> + Clone + '_ {
        self.binned.cut(count);

        self.binned.newest_first()
    }

    /// The figures over the window: each pass's median, in the stack's order, and the GPU's median frame against
    /// `frame_median_nanos`, the frame chart's median cycle.
    pub fn figures(&self, frame_median_nanos: u64) -> GpuFigures {
        let passes = self
            .labels
            .shown()
            .enumerate()
            .map(|(index, label)| {
                let median_nanos = median(self.binned.window().map(|timed| timed.stack.nanos[index]).collect());
                PassFigure { label: String::from(label), median_nanos }
            })
            .collect();
        let total_nanos = median(self.binned.window().map(|timed| timed.total_nanos).collect());

        GpuFigures::of(passes, total_nanos, frame_median_nanos)
    }
}
