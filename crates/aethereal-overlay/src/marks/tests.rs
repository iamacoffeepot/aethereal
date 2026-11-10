use super::{MOST_MARKS, Marks};
use crate::frames::WINDOW_NANOS;

/// A mark stays while it is within the chart's sixty seconds of the newest moment and leaves once it is not, and
/// the marks kept are bounded, so a mark that never leaves the chart, or leaves early, is caught.
#[test]
fn marks_older_than_the_chart_leave_it() {
    let mut marks = Marks::default();
    marks.pin(1_000);
    marks.pin(2_000);

    marks.keep_since(WINDOW_NANOS + 1_500);
    let kept: Vec<u64> = marks.times().collect();
    for at in 0..u64::from(MOST_MARKS) * 2 {
        marks.pin(WINDOW_NANOS + at);
    }

    assert_eq!(kept, [2_000]);
    assert_eq!(marks.times().count(), MOST_MARKS as usize);
    assert_eq!(marks.times().next(), Some(WINDOW_NANOS + u64::from(MOST_MARKS)));
}

/// A mark told between cycles is pinned at the next cycle's time, not the last one heard, which may be long past
/// while the overlay was hidden; and it is pinned once, so a mark stamped at a stale time, or twice, is caught.
#[test]
fn a_mark_waits_for_the_next_tick() {
    let mut marks = Marks::default();

    marks.wait(String::from("hello"));
    let stamped = marks.stamp(5_000);
    let again = marks.stamp(6_000);

    assert_eq!(stamped, ["hello"]);
    assert!(again.is_empty());
    assert_eq!(marks.times().collect::<Vec<u64>>(), [5_000]);
}
