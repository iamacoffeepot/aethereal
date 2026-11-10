//! Which windows are drawn and presented as the compositor asks, and how long they have waited on it.

use super::*;
use crate::FrameRate;

#[test]
fn a_fifo_window_holds_the_frame_drawn_ahead_until_the_compositor_asks_and_is_drawn_no_further() {
    // Catches a `fifo` frame presented before the compositor asked, whose present waits on the event loop's thread
    // until a covered window is uncovered; a frame drawn only after the ask, which lands a display frame late; and a
    // second frame drawn over the one held.
    let presented = Redraw::presented(Presenting::Fifo, 7);
    let held = presented.held();

    assert_eq!(presented, Redraw::Awaiting { presented_nanos: 7 });
    assert_eq!(presented.drew(), Drew::Hold, "drawn ahead of the ask, and held");
    assert!(drawn(Visible::Shown, true, presented), "drawn ahead");
    assert_eq!(held, Redraw::Drawn { presented_nanos: 7 });
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
        assert_eq!(Redraw::presented(presenting, 7), Redraw::Due, "{presenting}");
    }
}

#[test]
fn the_windows_are_silent_only_while_every_one_awaits_and_since_the_last_present() {
    // Catches undrawn cycles run while one window is still drawn, and a silence dated from the first window's present
    // rather than the last, which would begin them early.
    let awaiting = |presented_nanos| Redraw::Awaiting { presented_nanos };
    let holding = |presented_nanos| Redraw::Drawn { presented_nanos };

    assert_eq!(Silence::of([]), Silence::Heard, "none shows");
    assert_eq!(Silence::of([awaiting(5), Redraw::Due]), Silence::Heard);
    assert_eq!(Silence::of([awaiting(5), holding(9), awaiting(7)]), Silence::Since { nanos: 9 });
}

#[test]
fn a_held_cycle_waits_on_a_window_holding_a_frame_lately_and_runs_undrawn_past_a_silent_one() {
    // Catches a held cycle run while a window still holds the frame drawn ahead, which would leave every other frame
    // of a clip recorded `fifo` undrawn, and one that waits on a covered window until it is uncovered.
    let budget = 10;
    let lately = Redraw::Drawn { presented_nanos: 100 };
    let silent = Redraw::Drawn { presented_nanos: 20 };
    let awaiting = Redraw::Awaiting { presented_nanos: 100 };

    assert_eq!(Hold::of([Redraw::Due, awaiting], budget, 120), Hold::RunNow, "both are drawn");
    assert_eq!(Hold::of([lately, Redraw::Due], budget, 120), Hold::WaitUntil { nanos: 160 });
    assert_eq!(Hold::of([lately], budget, 160), Hold::RunNow, "it has waited six frames");
    assert_eq!(Hold::of([silent, Redraw::Due], budget, 120), Hold::RunNow);
}
