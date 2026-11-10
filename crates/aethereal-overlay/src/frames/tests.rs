use super::bin::{BIN_SAMPLES, Bin, Candle};
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

/// The candle of each column of a chart of `count` whose span ended before the newest's, by its span.
fn finished(times: &mut FrameTimes, count: u32) -> BTreeMap<u64, Candle> {
    let newest = times.slide(count).newest;

    times.columns(count).filter(|column| column.span < newest).map(|column| (column.span, column.candle)).collect()
}

/// The candle of a column of cycles each `delta_nanos` long, in order.
fn candle(deltas: impl IntoIterator<Item = u64>) -> (Candle, usize) {
    let mut deltas = deltas.into_iter();
    let mut bin = Bin::first(0, deltas.next().unwrap_or_default());
    for delta_nanos in deltas {
        bin.take(delta_nanos);
    }

    (bin.candle(), bin.kept())
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
/// fastest and the slowest of the cycles in it, so a column cut from the newest cycle rather than from the program's
/// start, or a spike averaged away, is caught.
#[test]
fn columns_are_spans_of_the_programs_time_and_keep_the_slowest() {
    // Sixty columns of a second each. The cycles end 5, 6.2, 6.5, and 6.6 seconds in: the first in the sixth second's
    // column; the other three in the seventh's, the slowest of them 1.2 seconds and the fastest 0.1.
    let mut frames = times([5_000 * MILLI, 1_200 * MILLI, 300 * MILLI, 100 * MILLI]);

    let columns: Vec<Column> = frames.columns(60).collect();

    let spans: Vec<u64> = columns.iter().map(|column| column.span).collect();
    assert_eq!(spans, vec![6, 5]);
    assert_eq!((columns[0].candle.fastest, columns[0].candle.slowest), (100 * MILLI, 1_200 * MILLI));
    assert_eq!(columns[1].candle, Candle::flat(5_000 * MILLI));
}

/// A chart cut into another count of columns, as the window is resized, is binned again from the cycles kept: the same
/// columns as times that were only ever cut into that count, so columns kept at the old span, drawn at the new one's
/// places, are caught.
#[test]
fn a_chart_cut_again_bins_the_cycles_again() {
    let cycle = || [10, 30, 17, 90].map(|millis| millis * MILLI).into_iter().cycle();
    let mut resized = times(cycle().take(400));
    let mut fresh = resized.clone();
    let _ = resized.columns(60).count();

    let again: Vec<Column> = resized.columns(25).collect();
    let only: Vec<Column> = fresh.columns(25).collect();

    assert_eq!(again, only);
    assert!(again.len() > 2, "{again:?} are too few to tell");
}

/// A column's candle is its fastest and slowest cycle and the nearest ranks of its quartiles and median, and a column
/// of one cycle is that cycle throughout, so a quartile read off by one rank, or a candle of one cycle that loses it,
/// is caught.
#[test]
fn a_candle_is_the_ranks_of_its_cycles() {
    // Sixteen cycles of 1 to 16 milliseconds, as a column at 60 Hz holds, out of order.
    let (sixteen, _) = candle((1..=16).rev().map(|millis| millis * MILLI));
    let (one, _) = candle([7 * MILLI]);

    let expected =
        Candle { fastest: MILLI, lower: 4 * MILLI, median: 8 * MILLI, upper: 12 * MILLI, slowest: 16 * MILLI };
    assert_eq!(sixteen, expected);
    assert_eq!(one, Candle::flat(7 * MILLI));
}

/// A column of more cycles than its sample holds keeps its fastest and slowest exactly, a spike among the cycles the
/// sample passes over included, keeps no more than its sample's room, and its quartiles and median stay where the
/// cycles' are, so a sample that grows without bound, drops the spike, or keeps only the column's first cycles is
/// caught.
#[test]
fn a_column_past_its_sample_keeps_its_ends_exactly() {
    // A thousand cycles of 1 to 1000 milliseconds, the last, which the sample passes over, a five-second spike, and
    // the fourth, also passed over, half a millisecond.
    let deltas = (1..=1_000_u64).map(|millis| match millis {
        4 => MILLI / 2,
        1_000 => 5_000 * MILLI,
        _ => millis * MILLI,
    });

    let (candle, kept) = candle(deltas);

    assert!(kept <= BIN_SAMPLES, "{kept} cycles kept");
    assert_eq!((candle.fastest, candle.slowest), (MILLI / 2, 5_000 * MILLI));
    let near = |nanos: u64, millis: u64| nanos.abs_diff(millis * MILLI) <= 20 * MILLI;
    let quartiles = near(candle.lower, 250) && near(candle.median, 500) && near(candle.upper, 750);
    assert!(quartiles, "{candle:?} is not near the cycles' quartiles");
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
    let kept: BTreeMap<u64, Candle> = after.into_iter().filter(|(span, _)| before.contains_key(span)).collect();
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
