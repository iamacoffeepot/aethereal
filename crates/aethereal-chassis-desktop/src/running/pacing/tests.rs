//! When frames are drawn and due, on stated times.

use super::*;
use crate::FrameRate;
use crate::redrawing::Silence;

#[test]
fn a_capped_frame_asked_between_the_points_of_its_grid_waits_for_the_next() {
    // Catches capped frames drawn whenever one is asked for, as a resize asks, past the rate, and a loop told to wait
    // on events alone, which would never ask for the next frame.
    let Ok(frames_per_second) = FrameRate::new(100) else {
        panic!("a hundred frames a second is a rate");
    };
    let mut pacing = Pacing::new(Presenting::Capped { frames_per_second }, 0);

    let at_start = (pacing.due(0), pacing.draws(0));
    let after = pacing.due(1);
    let between = pacing.draws(4_000_000);
    let on_grid = (pacing.due(10_000_000), pacing.draws(10_000_000));

    assert_eq!(at_start, (Due::Now, true));
    assert_eq!(after, Due::At(10_000_000), "a hundredth of a second on");
    assert!(!between, "a frame asked for between the points is not drawn");
    assert_eq!(on_grid, (Due::Now, true));
    assert!(!pacing.follows());
}

#[test]
fn uncapped_and_fifo_frames_are_each_drawn_and_follow_one_another() {
    // Catches frames that follow one another gated by a timer, which would cap a mode that has no rate.
    for presenting in [Presenting::Fifo, Presenting::Uncapped] {
        let mut pacing = Pacing::new(presenting, 0);

        let drawn = [pacing.draws(0), pacing.draws(1)];

        assert_eq!(drawn, [true, true], "{presenting}");
        assert_eq!(pacing.due(1), Due::Asked, "{presenting}");
        assert!(pacing.follows(), "{presenting}");
    }
}

#[test]
fn windows_silent_past_a_few_frames_run_cycles_undrawn_on_the_budget_and_one_heard_stops_them() {
    // Catches a covered window stopping the program's cycles for as long as it is covered, undrawn cycles begun while
    // frames are merely late, a grid that drifts from the budget, and cycles run on once the compositor asks again.
    let budget = 10;
    let mut undrawn = Undrawn::Unplanned;
    let silent = Silence::Since { nanos: 100 };

    let late = undrawn.at(silent, budget, 120);
    let first = undrawn.at(silent, budget, 160);
    let between = undrawn.at(silent, budget, 165);
    let second = undrawn.at(silent, budget, 171);
    let heard = undrawn.at(Silence::Heard, budget, 172);
    let afresh = undrawn.at(Silence::Since { nanos: 200 }, budget, 201);

    assert_eq!(late, Undrawing::WaitUntil { nanos: 160 }, "silent for six frames from the last present");
    assert_eq!(first, Undrawing::CycleNow { next_nanos: 170 });
    assert_eq!(between, Undrawing::WaitUntil { nanos: 170 });
    assert_eq!(second, Undrawing::CycleNow { next_nanos: 180 });
    assert_eq!(heard, Undrawing::Heard);
    assert_eq!(afresh, Undrawing::WaitUntil { nanos: 260 }, "a new silence plans from its own present");
}
