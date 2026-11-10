//! The figures beneath the GPU's graph (ADR-0053: The three stops): each pass's median over the window, written in its
//! pass's colour so the line doubles as the graph's legend, and the share of a frame the GPU was busy, its median frame
//! against the frame chart's median cycle.

use crate::colour::{BONE, PASS_SWATCHES, STONE};
use aethereal_data::Plain;
use aethereal_render::text::Run;

/// What a pass's figure is shown to: a hundredth of a millisecond, as a pass takes a fraction of one.
const SHOWN_NANOS: u64 = 10_000;

/// One pass's figure: its label as shown, and its median over the window.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub struct PassFigure {
    pub label: String,
    pub median_nanos: u64,
}

/// How busy the GPU was over the window.
#[derive(Plain, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    /// No cycle has been timed, so there is no frame to measure it against.
    #[default]
    Unknown,
    /// Its median frame, as a whole percentage of the median cycle.
    Share { percent: u64 },
}

/// The figures as they were last worked out: each pass's, in the stack's order, and how busy the GPU was.
#[derive(Plain, Debug, Default, Clone, PartialEq, Eq)]
pub struct GpuFigures {
    passes: Vec<PassFigure>,
    busy: Busy,
}

impl GpuFigures {
    /// The figures of `passes`, with the GPU's median frame `total_nanos` against the median cycle
    /// `frame_median_nanos`.
    pub fn of(passes: Vec<PassFigure>, total_nanos: u64, frame_median_nanos: u64) -> Self {
        let busy = match frame_median_nanos {
            0 => Busy::Unknown,
            cycle => Busy::Share { percent: (total_nanos.saturating_mul(100) + cycle / 2) / cycle },
        };

        Self { passes, busy }
    }

    /// The runs of the text that shows them: `main 2.31  overlay 0.12 ms  gpu busy 14%`, each pass's label and figure
    /// in its pass's colour, and the rest in stone and bone. Before any frame is timed, that none has been.
    pub fn runs(&self) -> Vec<Run> {
        if self.passes.is_empty() {
            return vec![Run::new("gpu  no frame timed yet", STONE.opaque())];
        }
        let mut runs: Vec<Run> = self
            .passes
            .iter()
            .enumerate()
            .map(|(index, pass)| {
                let gap = if index == 0 {
                    ""
                } else {
                    "  "
                };
                let swatch = PASS_SWATCHES[index % PASS_SWATCHES.len()];
                Run::new(&format!("{gap}{} {}", pass.label, millis(pass.median_nanos)), swatch.opaque())
            })
            .collect();
        let busy = match self.busy {
            Busy::Unknown => String::from("-"),
            Busy::Share { percent } => format!("{percent}%"),
        };
        runs.push(Run::new(" ms  gpu busy ", STONE.opaque()));
        runs.push(Run::new(&busy, BONE.opaque()));

        runs
    }
}

/// `nanos` in milliseconds to the nearest hundredth: `0.12`.
fn millis(nanos: u64) -> String {
    let hundredths = nanos.saturating_add(SHOWN_NANOS / 2) / SHOWN_NANOS;

    format!("{}.{:02}", hundredths / 100, hundredths % 100)
}

/// The one line the graph's place shows on a device without timestamps.
pub fn unsupported() -> Vec<Run> {
    vec![Run::new("GPU timing not supported on this device", STONE.opaque())]
}
