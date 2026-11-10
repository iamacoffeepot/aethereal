use super::{Column, Figures, FrameTimes, Slide, WINDOW_NANOS};
use aethereal_actor::Tick;
use core::iter;
use std::collections::BTreeMap;

/// A millisecond, in nanoseconds.
const MILLI: u64 = 1_000_000;

/// The frame times of cycles each `delta_nanos` long, the first ending at its own length.
fn times(deltas: impl IntoIterator<Item = u64>) -> FrameTimes {
    let mut times = FrameTimes::default();
    extend(&mut times, deltas);

    times
}

/// Pushes cycles each `delta_nanos` long onto `times`, the first ending its own length after the newest.
fn extend(times: &mut FrameTimes, deltas: impl IntoIterator<Item = u64>) {
    let mut elapsed_nanos = times.newest_nanos();
    for delta_nanos in deltas {
        elapsed_nanos += delta_nanos;
        times.push(&Tick { delta_nanos, elapsed_nanos });
    }
}

/// The longest cycle of each column of a chart of `count` whose span ended before the newest's, by its span.
fn finished(times: &FrameTimes, count: u32) -> BTreeMap<u64, u64> {
    let newest = times.slide(count).newest;

    times
        .columns(count)
        .filter(|column| column.span < newest)
        .map(|column| (column.span, column.longest_nanos))
        .collect()
}

/// The figures are the nearest ranks of the cycles kept, so a percentile read off by one rank, or the median taken
/// as the mean, is caught.
#[test]
fn figures_are_nearest_ranks() {
    let figures = times((1..=100).map(|millis| millis * MILLI)).figures();

    let expected = Figures { median: 50 * MILLI, p90: 90 * MILLI, p99: 99 * MILLI, longest: 100 * MILLI };
    assert_eq!(figures, expected);
}

/// A cycle more than sixty seconds older than the newest leaves the figures, so a spike long gone stops showing as
/// the longest.
#[test]
fn cycles_older_than_the_window_are_let_go() {
    let spike = 500 * MILLI;
    let steady = 20 * MILLI;
    let after = WINDOW_NANOS / steady;

    let figures = times(iter::once(spike).chain((0..after).map(|_| steady))).figures();

    assert_eq!(figures.longest, steady);
}

/// Figures are rounded to the tenth of a millisecond they show, so two that show the same compare equal and the text
/// is not written again for nothing.
#[test]
fn figures_are_rounded_as_shown() {
    let below = times([16_649_999]).figures();
    let above = times([16_650_000]).figures();

    assert_eq!(below.median, 16_600_000);
    assert_eq!(above.median, 16_700_000);
}

/// Each cycle falls in the column of the span of the program's time it ended in, newest first, and a column keeps the
/// longest of the cycles in it, so a column cut from the newest cycle rather than from the program's start, or a spike
/// averaged away, is caught.
#[test]
fn columns_are_spans_of_the_programs_time_and_keep_the_longest() {
    // Sixty columns of a second each. The cycles end 5, 6.2, 6.5, and 6.6 seconds in: the first in the sixth second's
    // column; the other three in the seventh's, the longest of them 1.2 seconds.
    let frames = times([5_000 * MILLI, 1_200 * MILLI, 300 * MILLI, 100 * MILLI]);

    let columns: Vec<Column> = frames.columns(60).collect();

    let newest = Column { span: 6, longest_nanos: 1_200 * MILLI };
    let before = Column { span: 5, longest_nanos: 5_000 * MILLI };
    assert_eq!(columns, vec![newest, before]);
}

/// A column whose span has passed keeps its height as the program's time moves on by part of a span, across the start
/// of the next, so columns cut again from the newest cycle, which hops a spike between columns as the chart moves (the
/// jitter seen live), is caught.
#[test]
fn a_finished_column_keeps_its_height_as_time_moves_on() {
    // Sixty columns of a second each, under cycles of 30, 50, 70, and 90 milliseconds over and over, which fall
    // unevenly across the columns: 10.8 seconds of them, then 0.32 seconds more, into the twelfth second.
    let cycle = || [30, 50, 70, 90].map(|millis| millis * MILLI).into_iter().cycle();
    let mut frames = times(cycle().take(180));
    let before = finished(&frames, 60);

    extend(&mut frames, cycle().take(6));
    let after = finished(&frames, 60);

    assert_eq!(before.len(), 10, "{before:?} are not the ten seconds before the newest");
    let kept: BTreeMap<u64, u64> = after.into_iter().filter(|(span, _)| before.contains_key(span)).collect();
    assert_eq!(kept, before);
}

/// The columns slide left by how far the newest cycle came through its span, so a chart that hops a whole column at
/// once, or measures the slide from the wrong end of the span, is caught.
#[test]
fn the_slide_is_how_far_through_its_span_the_newest_cycle_came() {
    // Sixty columns of a second each; the newest cycle ends 6.25 seconds in, a quarter of the way through the
    // seventh second.
    let frames = times([6_250 * MILLI]);

    assert_eq!(frames.slide(60), Slide { newest: 6, through: 0.25, span_nanos: 1_000_000_000 });
}
