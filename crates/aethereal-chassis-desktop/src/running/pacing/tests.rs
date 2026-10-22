//! When frames are drawn and due, on stated times.

use super::*;
use crate::FrameRate;

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
