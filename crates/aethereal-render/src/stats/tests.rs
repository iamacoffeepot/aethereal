//! Frame statistics' own arithmetic: a display's refresh as a frame's nanoseconds, and each pass's time beside its
//! name.

use super::budget::refresh_nanos;
use super::*;
use crate::frame::FrameId;
use crate::pass::{Compute, Main};

#[test]
fn a_refresh_in_millihertz_is_a_second_over_it_and_none_is_the_fallback() {
    // Catches a refresh read in hertz rather than millihertz, which is a thousand times off, and a display that reports
    // none dividing by zero.
    assert_eq!(refresh_nanos(60_000), 16_666_666);
    assert_eq!(refresh_nanos(143_998), 6_944_540);
    assert_eq!(refresh_nanos(0), refresh_nanos(FALLBACK_REFRESH_MILLIHERTZ));
    assert_eq!(FrameBudget::UNTOLD, FrameBudget::Reference { nanos: 16_666_666 });
}

#[test]
fn each_pass_s_time_is_told_beside_its_own_frame_and_name() {
    // Catches a pass's time paired with the pass before or after it, and a time told for a pass that did not run
    // when fewer times were mapped back than passes were named.
    let texture = Frame::Texture(FrameId { id: 7 });
    let ran = |pass, label: &str| Ran { frame: texture, pass, label: String::from(label) };
    let names = || vec![ran(PassName::of::<Compute>(), "compute"), ran(PassName::of::<Main>(), "main")];

    let told = named(names(), &[30, 70]);
    let short = named(names(), &[30]);

    let pass = |pass, label: &str, nanos| PassTime { frame: texture, pass, label: String::from(label), nanos };
    assert_eq!(told, [pass(PassName::of::<Compute>(), "compute", 30), pass(PassName::of::<Main>(), "main", 70)]);
    assert_eq!(short, [pass(PassName::of::<Compute>(), "compute", 30)]);
}
