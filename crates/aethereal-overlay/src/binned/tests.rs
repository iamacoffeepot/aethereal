use super::{Accrue, Binned, Spanned};
use crate::frames::WINDOW_NANOS;

/// A second, in nanoseconds.
const SECOND: u64 = 1_000_000_000;

/// A column that is the sum of its samples, so which samples fell in it reads off its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sum(u64);

impl Accrue for Sum {
    type Sample = u64;
    type Column = u64;

    fn first(sample: u64) -> Self {
        Self(sample)
    }

    fn take(&mut self, sample: u64) {
        self.0 += sample;
    }

    fn column(&self) -> u64 {
        self.0
    }
}

/// The columns of `binned` cut as a chart of `count`, newest first.
fn columns(binned: &mut Binned<Sum>, count: u32) -> Vec<Spanned<u64>> {
    binned.cut(count);

    binned.newest_first().collect()
}

/// A sample told late, its cycle's span before the open column's, as the GPU's times come a frame or more after their
/// frame, is taken into the open column, so a late sample that closes the newest column and opens an older one after
/// it, which draws two columns at one place and loses the newer, is caught.
#[test]
fn a_sample_told_late_joins_the_open_column() {
    let mut binned = Binned::<Sum>::default();
    binned.cut(60);

    binned.push(SECOND / 2, 1);
    binned.push(SECOND + SECOND / 2, 10);
    binned.push(SECOND - 1, 100);
    binned.push(2 * SECOND + 1, 1_000);

    let spans: Vec<(u64, u64)> =
        columns(&mut binned, 60).iter().map(|spanned| (spanned.span, spanned.column)).collect();
    assert_eq!(spans, [(2, 1_000), (1, 110), (0, 1)]);
}

/// A chart cut into another count, as the window is resized, is binned again from every sample kept into columns of
/// the new span, so a recut that keeps the old columns, or loses the samples before it, is caught.
#[test]
fn a_chart_cut_again_bins_every_sample_kept_again() {
    let mut binned = Binned::<Sum>::default();
    binned.cut(60);
    for second in 0..10 {
        binned.push(second * SECOND + 1, 1);
    }

    let thirty = columns(&mut binned, 30);

    // Thirty columns of two seconds each: five, two samples in each.
    let spans: Vec<(u64, u64)> = thirty.iter().map(|spanned| (spanned.span, spanned.column)).collect();
    assert_eq!(spans, [(4, 2), (3, 2), (2, 2), (1, 2), (0, 2)]);
}

/// The window is the samples of the last minute before the newest, though three minutes are kept for a recut, so a
/// figure worked out over every sample kept is caught.
#[test]
fn the_window_is_the_last_minute() {
    let mut binned = Binned::<Sum>::default();
    for second in 0..120 {
        binned.push(second * SECOND, second);
    }

    let window: Vec<u64> = binned.window().collect();

    assert_eq!(window.len(), usize::try_from(WINDOW_NANOS / SECOND).unwrap_or_default());
    assert_eq!((window.first(), window.last()), (Some(&119), Some(&60)));
}
