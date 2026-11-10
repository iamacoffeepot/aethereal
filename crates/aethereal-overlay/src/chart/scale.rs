//! How high the chart stands a cycle (ADR-0053: The three stops): on a scale logarithmic in its time and relative to
//! the budget, from a quarter of the budget at the chart's foot to thirty-two budgets at its top, seven doublings, so
//! the budget stands two sevenths of the way up and each doubling of a cycle's time a seventh higher.
//!
//! A frame of a 240 Hz budget and one of a 60 Hz budget stand at the same height against their own, and a spike of a
//! hundred budgets reads beside a column of healthy cycles, which a linear scale would press flat at its foot.

/// How many doublings of a cycle's time the chart's height spans.
const DOUBLINGS_TALL: f64 = 7.0;

/// How many doublings below the budget the chart's foot is: a quarter of it.
const DOUBLINGS_UNDER_BUDGET: f64 = 2.0;

/// How far up the chart, from its foot at zero to its top at one, a cycle `nanos` long stands against `budget_nanos`:
/// a cycle of a quarter of the budget or less at the foot, one of thirty-two budgets or more at the top, and one of
/// the budget two sevenths of the way up.
// Nanoseconds of at most some minutes, which an f64 holds closely enough to draw; and a share of the chart from nothing
// to one, which an f32 holds closely enough too.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
pub fn height_fraction(nanos: u64, budget_nanos: u64) -> f32 {
    let budgets = nanos as f64 / budget_nanos.max(1) as f64;

    ((budgets.log2() + DOUBLINGS_UNDER_BUDGET) / DOUBLINGS_TALL).clamp(0.0, 1.0) as f32
}
