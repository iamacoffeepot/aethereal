//! The mode's tests: what winit says of a window read as the one mode it is shown in.

use super::read;
use aethereal_window::Mode;

#[test]
fn a_window_fullscreen_reads_as_fullscreen_whether_or_not_it_is_maximized_beneath() {
    // Catches a window maximized, then made fullscreen, read as maximized on a platform that says it is both, so its
    // subscribers never hear it went fullscreen and a toggle back to windowed asks for the mode it already has.
    assert_eq!(read(true, true), Mode::Fullscreen);
    assert_eq!(read(true, false), Mode::Fullscreen);
    assert_eq!(read(false, true), Mode::Maximized);
    assert_eq!(read(false, false), Mode::Windowed);
}
