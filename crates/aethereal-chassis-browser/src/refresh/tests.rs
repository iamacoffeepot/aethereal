//! The refresh measured from a page's animation frames.

use super::*;

/// The budget said after frames `apart` nanoseconds apart from 0, each in turn: the last said, if any.
fn said(refresh: &mut Refresh, apart: &[u64]) -> Option<FrameBudget> {
    let mut at = 0;
    let mut said = refresh.frame(at);
    for interval in apart {
        at += interval;
        said = refresh.frame(at).or(said);
    }

    said
}

#[test]
fn the_budget_is_the_shortest_interval_and_a_late_frame_does_not_lengthen_it() {
    // Catches a refresh measured as the mean, which a program over its budget would drag out so that it never looked
    // over it, and a frame on a 144 Hz display measured as 60 Hz.
    let mut refresh = Refresh::new();
    let at_144 = [6_944_444, 6_944_444, 13_888_888, 6_944_444];

    assert_eq!(said(&mut refresh, &at_144), Some(FrameBudget::Limit { nanos: 6_944_444 }));
}

#[test]
fn jitter_is_not_said_again_and_a_page_hidden_is_not_measured() {
    // Catches the front told a new budget at every frame of a display's jitter, and a page shown again after a
    // second hidden read as a refresh of a second.
    let mut refresh = Refresh::new();
    assert_eq!(said(&mut refresh, &[6_944_444]), Some(FrameBudget::Limit { nanos: 6_944_444 }));

    assert_eq!(said(&mut refresh, &[6_900_000, 6_990_000, 5_000_000_000]), None);
}
