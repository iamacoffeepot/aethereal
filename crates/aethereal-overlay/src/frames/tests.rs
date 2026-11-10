use super::bin::{BIN_SAMPLES, Bin};
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

/// Each column of a chart of `count` whose span ended before the newest's, by its span.
fn finished(times: &mut FrameTimes, count: u32) -> BTreeMap<u64, Column> {
    let newest = times.slide(count).newest;

    times.columns(count).filter(|column| column.span < newest).map(|column| (column.span, column)).collect()
}

/// The column of cycles each `delta_nanos` long, in order, and how many cycles its sample kept.
fn column(deltas: impl IntoIterator<Item = u64>) -> (Column, usize) {
    let mut deltas = deltas.into_iter();
    let mut bin = Bin::first(0, deltas.next().unwrap_or_default());
    for delta_nanos in deltas {
        bin.take(delta_nanos);
    }

    (bin.column(), bin.kept())
}

/// A column of one cycle `nanos` long, of `span`.
const fn flat(span: u64, nanos: u64) -> Column {
    Column { span, fastest_nanos: nanos, median_nanos: nanos, slowest_nanos: nanos }
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

/// The gutter's line beside the chart is `frame` and the median in milliseconds, flush right in the gutter as the GPU
/// graph's top and the rows' values are, so a median written in the wrong unit, or out of line with the gutter's other
/// values, is caught.
#[test]
fn the_gutter_says_the_chart_s_median() {
    let figures = times([4_200_000, 4_200_000, 16_700_000]).figures();

    let line: String = figures.gutter_runs().iter().map(|run| run.text.as_str()).collect();

    assert_eq!(line, "frame         4.2 ms");
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
/// fastest and the slowest of the cycles in it, so a column cut from the newest cycle rather than from the program's
/// start, or a spike averaged away, is caught.
#[test]
fn columns_are_spans_of_the_programs_time_and_keep_the_slowest() {
    // Sixty columns of a second each. The cycles end 5, 6.2, 6.5, and 6.6 seconds in: the first in the sixth second's
    // column; the other three in the seventh's, the slowest of them 1.2 seconds, the median 0.3, and the fastest 0.1.
    let mut frames = times([5_000 * MILLI, 1_200 * MILLI, 300 * MILLI, 100 * MILLI]);

    let columns: Vec<Column> = frames.columns(60).collect();

    let spans: Vec<u64> = columns.iter().map(|column| column.span).collect();
    assert_eq!(spans, vec![6, 5]);
    let newest =
        Column { span: 6, fastest_nanos: 100 * MILLI, median_nanos: 300 * MILLI, slowest_nanos: 1_200 * MILLI };
    assert_eq!(columns[0], newest);
    assert_eq!(columns[1], flat(5, 5_000 * MILLI));
}

/// A chart cut into another count of columns, as the window is resized or maximized, is binned again from the cycles
/// kept, and goes on taking cycles at its new count: the same columns as times that were only ever cut into that
/// count, each column's slowest no slower than the slowest cycle taken, so columns kept at the old span, drawn at the
/// new one's places, or a recut that takes a cycle twice, are caught.
#[test]
fn a_chart_cut_again_mid_stream_bins_the_cycles_again() {
    let cycle = || [4, 5, 4, 6].map(|millis| millis * MILLI).into_iter().cycle();
    let mut resized = times(cycle().take(4_000));
    let mut fresh = resized.clone();
    let _ = resized.columns(226).count();

    let _ = resized.columns(1_000).count();
    extend(&mut resized, cycle().take(2_000));
    extend(&mut fresh, cycle().take(2_000));
    let again: Vec<Column> = resized.columns(1_000).collect();
    let only: Vec<Column> = fresh.columns(1_000).collect();

    assert_eq!(again, only);
    assert!(again.len() > 2, "{again:?} are too few to tell");
    assert!(again.iter().all(|column| column.slowest_nanos <= 6 * MILLI), "{again:?} hold a cycle never taken");
}

/// A column is its fastest cycle, the nearest rank of its cycles' median, and its slowest cycle, and a column of one
/// cycle is that cycle for all three, so a median read off by one rank, taken as the mean, a fastest kept as the first
/// cycle, or a column of one cycle that loses it, is caught.
#[test]
fn a_column_is_its_fastest_median_and_slowest() {
    // Sixteen cycles of 16 down to 1 milliseconds, as a column at 60 Hz holds, out of order, and a spike among them.
    let (sixteen, _) = column((1..=16).rev().map(|millis| millis * MILLI).chain([90 * MILLI]));
    let (one, _) = column([7 * MILLI]);

    let expected = (MILLI, 9 * MILLI, 90 * MILLI);
    assert_eq!((sixteen.fastest_nanos, sixteen.median_nanos, sixteen.slowest_nanos), expected);
    assert_eq!(one, flat(0, 7 * MILLI));
}

/// A column of more cycles than its sample holds keeps its fastest and its slowest exactly, a dip and a spike among the
/// cycles the sample passes over included, keeps no more than its sample's room, and its median stays where the
/// cycles' is, so a sample that grows without bound, drops the dip or the spike, or keeps only the column's first
/// cycles is caught.
#[test]
fn a_column_past_its_sample_keeps_its_fastest_and_slowest_exactly() {
    // A thousand cycles of 2 to 1000 milliseconds, the 999th, which the sample passes over, a tenth of a millisecond,
    // and the last, which it passes over too, a five-second spike.
    let deltas = (2..=1_001_u64).map(|millis| match millis {
        1_000 => MILLI / 10,
        1_001 => 5_000 * MILLI,
        _ => millis * MILLI,
    });

    let (column, kept) = column(deltas);

    assert!(kept <= BIN_SAMPLES, "{kept} cycles kept");
    assert_eq!((column.fastest_nanos, column.slowest_nanos), (MILLI / 10, 5_000 * MILLI));
    let near = column.median_nanos.abs_diff(500 * MILLI) <= 20 * MILLI;
    assert!(near, "{column:?} is not near the cycles' median");
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
    let before = finished(&mut frames, 60);

    extend(&mut frames, cycle().take(6));
    let after = finished(&mut frames, 60);

    assert_eq!(before.len(), 10, "{before:?} are not the ten seconds before the newest");
    let kept: BTreeMap<u64, Column> = after.into_iter().filter(|(span, _)| before.contains_key(span)).collect();
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
