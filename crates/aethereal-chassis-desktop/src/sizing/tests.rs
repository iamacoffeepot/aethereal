//! The sizing's tests: a resize winit left undone reads as held, and one it applied, at the size asked or another,
//! does not.

use super::{held, physical};
use winit::dpi::{LogicalSize, PhysicalSize};

#[test]
fn a_size_answered_unchanged_and_not_as_asked_was_held() {
    // Catches a tiled window's ignored resize answered as resized, at the size it kept.
    let before = PhysicalSize::new(5054, 669);

    assert!(held(before, before, PhysicalSize::new(1920, 1080)));
}

#[test]
fn a_window_asked_for_its_own_size_or_cut_by_the_platform_was_not_held() {
    // Catches a resize to the size a window already has refused, as its answer is unchanged too, and a size the
    // platform cut to the screen refused for not being the size asked.
    let before = PhysicalSize::new(1920, 1080);

    assert!(!held(before, before, before));
    assert!(!held(before, PhysicalSize::new(2560, 1373), PhysicalSize::new(2560, 1440)));
}

#[test]
fn a_logical_size_is_rounded_to_whole_logical_pixels_before_scaling() {
    // Catches the asked size scaled before it is rounded, which on a screen of one and a half pixels to each differs
    // from winit's by a pixel and would read an applied resize as held.
    assert_eq!(physical(LogicalSize::new(100.4, 100.6), 1.5), PhysicalSize::new(150, 152));
}
