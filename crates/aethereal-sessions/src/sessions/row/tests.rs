use super::Grew;
use aethereal_actor::Time;

#[test]
fn a_row_s_time_keeps_how_long_ago_it_was_as_it_crosses_to_another_clock() {
    // Catches a row's time carried across the link as the hub's own, so an engine whose clock began elsewhere reads a
    // session's last activity hours off; one moved the wrong way, into the future; and one before the other clock
    // began wrapped around to centuries ahead.
    let hub_now = Time::from_nanos(1_000);
    let engine_now = Time::from_nanos(50_000);

    assert_eq!(Grew::At(Time::from_nanos(700)).rebased(hub_now, engine_now), Grew::At(Time::from_nanos(49_700)));
    assert_eq!(Grew::At(Time::from_nanos(700)).rebased(hub_now, Time::from_nanos(100)), Grew::At(Time::ZERO));
    assert_eq!(Grew::Unseen.rebased(hub_now, engine_now), Grew::Unseen);
}
