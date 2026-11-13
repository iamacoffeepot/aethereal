//! How the GPU's graph is drawn (ADR-0053: The three stops): beneath the frame chart, over the same columns, each a
//! stack of its passes from the graph's foot, each pass at its median over the column's frames, in its pass's colour.
//! The scale is linear, as stacked times are summed, and ranged to what is drawn, as a GPU profiler's is: nothing at
//! the foot, and at the top the tallest stack in the window rounded up to one, two, or five of a power of ten
//! nanoseconds, no less than ten microseconds (`GraphTop`). The top moves only between those steps, so it does not
//! flicker as the window slides. A pass that took any time stands at least a pixel tall, and a stack over the top is
//! cut there. The budget, dashed as the frame chart's, is drawn only where it falls under the top; the gutter beside
//! the graph says what the top is.
//!
//! ```text
//!   ███           the tallest stack, reaching the top        gpu       top 0.1 ms
//!   ███  - - - -  the budget, only when under the top
//!   ███  ██       a second pass, muted violet
//!   ███  ██       the first pass heard, slate blue, at the foot
//! ```

use super::Stack;
use crate::binned::Spanned;
use crate::chart::{Fill, dashes, left_of, push};
use crate::colour::{BONE, PASS_SWATCHES, STONE};
use crate::frames::Slide;
use crate::layout::{GUTTER_CHARACTERS, Scale};
use aethereal_data::Plain;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::stats::FrameBudget;
use aethereal_render::text::Run;

/// The least the graph's top is: ten microseconds, so an idle GPU's noise is not drawn the graph's full height.
const FLOOR_NANOS: u64 = 10_000;

/// The steps within each power of ten the top is rounded up to.
const STEPS: [u64; 3] = [1, 2, 5];

/// How many powers of ten the top climbs through from its floor: up to five of 10^18 nanoseconds, the last step a
/// u64 holds.
const DECADES: u32 = 15;

/// What the gutter calls the graph, its top written flush right in the rest of the gutter.
const LABEL: &str = "gpu";

/// A millisecond, in nanoseconds.
const MILLI_NANOS: u64 = 1_000_000;

/// The top of the GPU's graph: the tallest stack in the window, rounded up to one, two, or five of a power of ten
/// nanoseconds, and no less than [`FLOOR_NANOS`].
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphTop {
    nanos: u64,
}

impl Default for GraphTop {
    /// The floor, before any frame is timed.
    fn default() -> Self {
        Self { nanos: FLOOR_NANOS }
    }
}

impl GraphTop {
    /// The top over `columns`: the first step at or over the tallest of their stacks.
    pub fn of(columns: impl IntoIterator<Item = Spanned<Stack>>) -> Self {
        let tallest_nanos = columns.into_iter().map(|spanned| spanned.column.total_nanos()).max().unwrap_or(0);
        let mut decade = FLOOR_NANOS;
        let mut nanos = FLOOR_NANOS;
        for _ in 0..DECADES {
            for step in STEPS {
                nanos = decade.saturating_mul(step);
                let holds = nanos >= tallest_nanos;
                if holds {
                    return Self { nanos };
                }
            }
            decade = decade.saturating_mul(10);
        }

        Self { nanos }
    }

    /// The top, in nanoseconds.
    #[cfg(test)]
    pub const fn nanos(self) -> u64 {
        self.nanos
    }

    /// The runs of the gutter's line beside the graph: its label in stone, and its top in bone, flush right as the
    /// rows' values are: `gpu       top 0.1 ms`.
    pub fn runs(self) -> Vec<Run> {
        let width = GUTTER_CHARACTERS - LABEL.len();
        let value = format!("top {} ms", millis(self.nanos));

        vec![Run::new(LABEL, STONE.opaque()), Run::new(&format!("{value:>width$}"), BONE.opaque())]
    }
}

/// `nanos` in milliseconds, with as many places as it needs and no trailing zeros: `0.1`, `0.02`, `5`.
fn millis(nanos: u64) -> String {
    let (whole, rest) = (nanos / MILLI_NANOS, nanos % MILLI_NANOS);
    if rest == 0 {
        return whole.to_string();
    }
    let places = format!("{rest:06}");

    format!("{whole}.{}", places.trim_end_matches('0'))
}

/// Appends to `fills` the GPU's graph in `area` at `scale`, and answers the top it was drawn to: each of `columns`
/// placed at `slide` as the frame chart's column of its span is, cut to the graph's left and right edges, its passes
/// stacked from the foot in their order, each that took any time at least a pixel tall; and then the budget's dashed
/// line, where it falls under the top.
pub fn graph<I>(
    fills: &mut Vec<Fill>,
    area: Rect2,
    scale: Scale,
    slide: Slide,
    budget: FrameBudget,
    columns: I,
) -> GraphTop
where
    I: IntoIterator<Item = Spanned<Stack>>,
    I::IntoIter: Clone,
{
    let columns = columns.into_iter();
    let top = GraphTop::of(columns.clone());
    let width = scale.column_pixels();
    // Nanoseconds of at most some minutes, which an f64 holds closely enough to draw; and a share of the graph from
    // nothing to one, which an f32 holds closely enough too.
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    let height_of =
        |nanos: u64| ((nanos as f64 / top.nanos as f64).min(1.0) as f32).mul_add(-area.height(), area.max.y);

    for spanned in columns {
        let left = left_of(area, scale, slide, spanned.span);
        let (from, to) = (left.max(area.min.x), (left + width).min(area.max.x));
        let shows = from < to;
        if !shows {
            continue;
        }
        let mut beneath_nanos: u64 = 0;
        let mut bottom = area.max.y;
        for (index, nanos) in spanned.column.nanos.into_iter().enumerate() {
            beneath_nanos = beneath_nanos.saturating_add(nanos);
            if nanos == 0 {
                continue;
            }
            let upper = height_of(beneath_nanos).min(bottom - 1.0).max(area.min.y);
            let stands = upper < bottom;
            if !stands {
                continue;
            }
            let segment = Rect2 { min: Vec2::new(from, upper), max: Vec2::new(to, bottom) };
            push(fills, segment, PASS_SWATCHES[index % PASS_SWATCHES.len()].opaque());
            bottom = upper;
        }
    }

    let under_top = budget.nanos() <= top.nanos;
    if under_top {
        dashes(fills, area, scale, height_of(budget.nanos()).round(), budget);
    }

    top
}
