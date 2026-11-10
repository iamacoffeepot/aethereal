//! Which windows are drawn and presented as the compositor asks.

use super::*;
use crate::FrameRate;

#[test]
fn a_fifo_window_holds_the_frame_drawn_ahead_until_the_compositor_asks_and_is_drawn_no_further() {
    // Catches a `fifo` frame presented before the compositor asked, whose present waits on the event loop's thread
    // until a covered window is uncovered; a frame drawn only after the ask, which lands a display frame late; and a
    // second frame drawn over the one held.
    let presented = Redraw::presented(Presenting::Fifo);
    let held = presented.held();

    assert_eq!(presented, Redraw::Awaiting);
    assert_eq!(presented.drew(), Drew::Hold, "drawn ahead of the ask, and held");
    assert!(drawn(Visible::Shown, true, presented), "drawn ahead");
    assert_eq!(held, Redraw::Drawn);
    assert!(!drawn(Visible::Shown, true, held), "one frame ahead at most");
    assert_eq!(held.asked(), Asked::PresentHeld);
    assert_eq!(presented.asked(), Asked::Due, "nothing held to present");
    assert_eq!(Redraw::Due.drew(), Drew::Present);
    assert!(!drawn(Visible::Occluded, true, Redraw::Due));
    assert!(!drawn(Visible::Shown, false, Redraw::Due));
}

#[test]
fn a_window_presented_without_waiting_is_due_again_at_once() {
    // Catches `uncapped` and `capped` frames held to the compositor's frame callbacks, which would pace them to the
    // display, and stop them while the window is covered.
    let Ok(frames_per_second) = FrameRate::new(30) else {
        panic!("thirty frames a second is a rate");
    };

    for presenting in [Presenting::Uncapped, Presenting::Capped { frames_per_second }] {
        assert_eq!(Redraw::presented(presenting), Redraw::Due, "{presenting}");
    }
}
