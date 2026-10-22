//! The mode each presentation asks a surface for, and the rates a capped one holds.

use super::*;
use aethereal_data::{Budget, from_bytes, to_bytes};
use wgpu::PresentMode;

const SIXTY: Presenting = Presenting::Capped { frames_per_second: FrameRate(60) };

#[test]
fn a_presentation_that_never_waits_takes_immediate_then_mailbox() {
    // Catches a mode chosen in the wrong order, or a capped window presented with a mode that waits on the display.
    let everything = [PresentMode::Fifo, PresentMode::Mailbox, PresentMode::FifoRelaxed, PresentMode::Immediate];
    let no_tearing = [PresentMode::Fifo, PresentMode::Mailbox];

    for presenting in [Presenting::Uncapped, SIXTY] {
        assert_eq!(presenting.mode(&everything), Ok(PresentMode::Immediate), "{presenting}");
        assert_eq!(presenting.mode(&no_tearing), Ok(PresentMode::Mailbox), "{presenting}");
    }
    assert_eq!(Presenting::Fifo.mode(&everything), Ok(PresentMode::Fifo));
}

#[test]
fn a_mode_the_surface_lacks_is_refused_naming_what_it_offers() {
    // Catches a fall back to a mode with the other behaviour: a waiting mode for one that never waits, or the reverse.
    let waiting = [PresentMode::Fifo, PresentMode::FifoRelaxed, PresentMode::AutoVsync, PresentMode::AutoNoVsync];
    let never_waiting = [PresentMode::Mailbox, PresentMode::Immediate];

    let uncapped = Presenting::Uncapped.mode(&waiting);
    let capped = SIXTY.mode(&waiting);
    let fifo = Presenting::Fifo.mode(&never_waiting);

    assert_eq!(uncapped, Err(Unoffered { presenting: Presenting::Uncapped, offered: waiting.to_vec() }));
    assert_eq!(capped, Err(Unoffered { presenting: SIXTY, offered: waiting.to_vec() }));
    assert_eq!(fifo, Err(Unoffered { presenting: Presenting::Fifo, offered: never_waiting.to_vec() }));
    let said = Presenting::Fifo.mode(&never_waiting).map_err(|unoffered| unoffered.to_string());
    assert_eq!(said, Err(String::from("the window's surface cannot present `fifo`: it offers [Mailbox, Immediate]")));
}

#[test]
fn a_rate_holds_one_to_a_thousand_frames_a_second_however_it_is_read() {
    // Catches a bound one off at either end, and a decode that takes a count the constructor refuses.
    for frames_per_second in [0, 1, 1000, 1001] {
        let held = (1..=1000).contains(&frames_per_second);
        let made = FrameRate::new(frames_per_second);
        let bytes = to_bytes(&frames_per_second).map_err(|error| error.to_string());
        let read = bytes.map(|bytes| from_bytes::<FrameRate>(&bytes, &mut Budget::new(16)));

        assert_eq!(made.is_ok(), held, "{frames_per_second} made");
        let expected = if held {
            Ok(FrameRate(frames_per_second))
        } else {
            Err(DecodeError::OutOfRange { value: frames_per_second.into(), least: 1, most: 1000 })
        };
        assert_eq!(read, Ok(expected), "{frames_per_second} read");
    }
}
