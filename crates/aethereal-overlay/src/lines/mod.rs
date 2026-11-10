//! The four lines beneath the GPU's graph (ADR-0053: The three stops): draws, triangles, bytes uploaded, and GPU
//! memory, from each frame's statistics, kept over the frame chart's axis by the cycle each frame was drawn in and cut
//! into its columns, each the most of its frames, so a spike in one frame is not averaged away; each drawn as a row of
//! bars (`rows`), with its label and its latest value beside it in the gutter (`figures`).

mod figures;
mod rows;
#[cfg(test)]
mod tests;

pub use rows::rows;

use crate::binned::{Accrue, Binned, Spanned};
use aethereal_data::Plain;
use aethereal_render::stats::{FrameStats, GpuMemory};

/// How many lines there are, each a row and a column's record in it.
pub const LINES: u32 = 4;

/// [`LINES`], as the length of a frame's values.
const COUNT: usize = LINES as usize;

/// Which line, in the order the rows stand from the top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    /// The draws a frame recorded.
    Draws,
    /// The triangles those draws drew, where the CPU knows their count.
    Triangles,
    /// The bytes written to the GPU since the frame before.
    Uploaded,
    /// The GPU memory of everything the renderer made: each kind's bytes, summed.
    Memory,
}

impl Line {
    /// Every line, in the order the rows stand from the top.
    pub const ALL: [Self; COUNT] = [Self::Draws, Self::Triangles, Self::Uploaded, Self::Memory];
}

/// Each line's value in one frame, or the most of them over a column's frames, in [`Line::ALL`]'s order.
#[derive(Plain, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Values {
    values: [u64; COUNT],
}

impl Values {
    /// The values `stats` tell of their frame.
    pub fn of(stats: &FrameStats) -> Self {
        let GpuMemory { meshes_bytes, textures_bytes, targets_bytes, buffers_bytes, atlas_bytes } = stats.memory;
        let memory = [meshes_bytes, textures_bytes, targets_bytes, buffers_bytes, atlas_bytes]
            .into_iter()
            .fold(0_u64, u64::saturating_add);
        let drawn = stats.drawn;

        Self { values: [u64::from(drawn.draws), drawn.triangles, drawn.uploaded_bytes, memory] }
    }

    /// The value of `line`.
    pub const fn of_line(self, line: Line) -> u64 {
        self.values[line as usize]
    }
}

/// A column is the most of each line over its frames.
impl Accrue for Values {
    type Sample = Self;
    type Column = Self;

    fn first(values: Self) -> Self {
        values
    }

    fn take(&mut self, values: Self) {
        for (most, value) in self.values.iter_mut().zip(values.values) {
            *most = (*most).max(value);
        }
    }

    fn column(&self) -> Self {
        *self
    }
}

/// Each frame's values over the frame chart's axis, and the newest frame's, which the gutter shows: nothing of each
/// before a frame's statistics are heard.
#[derive(Plain, Debug, Default, Clone, PartialEq, Eq)]
pub struct FrameLines {
    binned: Binned<Values>,
    latest: Values,
}

impl FrameLines {
    /// Takes in the values of the frame `stats` tell of.
    pub fn push(&mut self, stats: &FrameStats) {
        self.latest = Values::of(stats);
        self.binned.push(stats.elapsed_nanos, self.latest);
    }

    /// The rows' columns for a chart of `count`, from the newest back, cut as the frame chart's are.
    pub fn columns(&mut self, count: u32) -> impl Iterator<Item = Spanned<Values>> + Clone + '_ {
        self.binned.cut(count);

        self.binned.newest_first()
    }

    /// The newest frame's values.
    pub const fn latest(&self) -> Values {
        self.latest
    }
}
