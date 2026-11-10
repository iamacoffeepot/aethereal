use super::{Column, Figures, FrameTimes, WINDOW_NANOS};
use aethereal_actor::Tick;
use core::iter;

/// A millisecond, in nanoseconds.
const MILLI: u64 = 1_000_000;

/// The frame times of cycles each `delta_nanos` long, the first ending at its own length.
fn times(deltas: impl IntoIterator<Item = u64>) -> FrameTimes {
    let mut times = FrameTimes::default();
    let mut elapsed_nanos = 0;
    for delta_nanos in deltas {
        elapsed_nanos += delta_nanos;
        times.push(&Tick { delta_nanos, elapsed_nanos });
    }

    times
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

/// The newest cycle is in the rightmost column, and a column keeps the longest of the cycles in it, so a chart drawn
/// backwards, or a spike averaged away, is caught.
#[test]
fn columns_put_the_newest_at_the_right_and_keep_the_longest() {
    // Sixty columns of a second each. The cycles end 5, 6.2, 6.5, and 6.6 seconds in: the first 1.6 seconds before
    // the newest, in the column before the last; the other three in the last, the longest of them 1.2 seconds.
    let frames = times([5_000 * MILLI, 1_200 * MILLI, 300 * MILLI, 100 * MILLI]);

    let columns = frames.columns(60);

    let newest = Column { at: 59, longest_nanos: 1_200 * MILLI };
    let before = Column { at: 58, longest_nanos: 5_000 * MILLI };
    assert_eq!(columns, vec![newest, before]);
}
