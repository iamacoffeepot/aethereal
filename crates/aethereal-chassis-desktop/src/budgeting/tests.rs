//! The frame budget for each way the desktop presents frames.

use super::*;
use crate::FrameRate;

#[test]
fn each_present_mode_aims_at_its_own_budget() {
    // Catches a cap read as the display's refresh, `fifo` drawn as a reference though the display paces it, `uncapped`
    // drawn as a limit though nothing holds the frames to it, and a refresh read in hertz rather than millihertz.
    let Ok(sixty) = FrameRate::new(60) else {
        panic!("60 frames a second is a rate");
    };
    let capped = Presenting::Capped { frames_per_second: sixty };
    let display = Some(144_000);

    assert_eq!(budget(capped, display), FrameBudget::Limit { nanos: 16_666_666 });
    assert_eq!(budget(Presenting::Fifo, display), FrameBudget::Limit { nanos: 6_944_444 });
    assert_eq!(budget(Presenting::Uncapped, display), FrameBudget::Reference { nanos: 6_944_444 });
}

#[test]
fn a_display_that_reports_no_refresh_is_taken_as_sixty_hertz() {
    // Catches a display with no refresh reported dividing by nothing, or giving a budget of none, which every frame
    // would be over.
    assert_eq!(budget(Presenting::Fifo, None), FrameBudget::Limit { nanos: 16_666_666 });
    assert_eq!(budget(Presenting::Uncapped, Some(0)), FrameBudget::Reference { nanos: 16_666_666 });
}
