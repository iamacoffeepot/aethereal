//! The figures beneath the GPU's graph (ADR-0053: The three stops): each pass's median over the window, written in its
//! pass's colour so the line doubles as the graph's legend, and the share of a frame the GPU was busy, its median frame
//! against the frame chart's median cycle.

use crate::colour::{Ink, PASSES};
use crate::console::Piece;
use aethereal_data::Plain;

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
    /// in its pass's colour, and the rest dim and in the text's ink. Before any frame is timed, that none has been.
    pub fn pieces(&self) -> Vec<Piece> {
        if self.passes.is_empty() {
            return vec![Piece::new("gpu  no frame timed yet", Ink::Dim)];
        }
        let mut runs: Vec<Piece> = self
            .passes
            .iter()
            .enumerate()
            .map(|(index, pass)| {
                let gap = if index == 0 {
                    ""
                } else {
                    "  "
                };
                let ink = PASSES[index % PASSES.len()];
                Piece::new(&format!("{gap}{} {}", pass.label, millis(pass.median_nanos)), ink)
            })
            .collect();
        let busy = match self.busy {
            Busy::Unknown => String::from("-"),
            Busy::Share { percent } => format!("{percent}%"),
        };
        runs.push(Piece::new(" ms  gpu busy ", Ink::Dim));
        runs.push(Piece::new(&busy, Ink::Text));

        runs
    }
}

/// `nanos` in milliseconds to the nearest hundredth: `0.12`.
fn millis(nanos: u64) -> String {
    let hundredths = nanos.saturating_add(SHOWN_NANOS / 2) / SHOWN_NANOS;

    format!("{}.{:02}", hundredths / 100, hundredths % 100)
}

/// The one line the graph's place shows on a device without timestamps.
pub fn unsupported() -> Vec<Piece> {
    vec![Piece::new("GPU timing not supported on this device", Ink::Dim)]
}
