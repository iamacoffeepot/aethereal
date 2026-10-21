//! A timer's grid, on stated times.

use super::*;

#[test]
fn a_cycle_is_due_at_the_start_and_at_each_period_after() {
    // Catches a timer due only once, due at every look, or due a period after it was looked at rather than after it
    // was due, which drifts later by however long each look waited.
    let mut timer = Timer::new(100, 10);

    let fired = [100, 105, 110, 119, 121].map(|now| timer.fires(now));

    assert_eq!(fired, [true, false, true, false, true]);
    assert_eq!(timer.due(), 130);
}

#[test]
fn a_cycle_fired_late_skips_what_it_missed_and_queues_none() {
    // Catches a timer that catches up by firing once for each period missed, or once at once, as a queued cycle
    // would: ADR-0020 makes the cycles fewer, never late in a queue.
    let mut timer = Timer::new(100, 10);
    timer.fires(100);

    let late = timer.fires(145);
    let after = timer.due();
    let at_once = timer.fires(146);
    let on_grid = timer.fires(170);

    assert!(late);
    assert_eq!(after, 150, "the first point of the grid after the late cycle");
    assert!(!at_once, "no cycle is queued for those missed");
    assert!(on_grid);
    assert_eq!(timer.due(), 180, "a cycle fired on a point of the grid is due again a period on");
}
