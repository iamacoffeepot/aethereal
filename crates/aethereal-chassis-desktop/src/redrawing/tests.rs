//! Which windows are drawn and presented as the compositor asks.

use super::*;
use crate::FrameRate;

#[test]
fn a_fifo_window_awaits_the_compositor_s_ask_and_is_drawn_no_further_until_it_comes() {
    // Catches a `fifo` frame drawn and presented before the compositor asked, whose present waits on the event loop's
    // thread until a covered window is uncovered, and a window hidden or of no size drawn.
    let presented = Redraw::presented(Presenting::Fifo);

    assert_eq!(presented, Redraw::Awaiting);
    assert!(!drawn(Visible::Shown, true, presented), "not drawn until asked");
    assert!(drawn(Visible::Shown, true, Redraw::Due));
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
