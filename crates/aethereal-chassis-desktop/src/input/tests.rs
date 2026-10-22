//! The translation's tests: winit's keys and wheel read as the window capability names and signs them.

use super::{code, key, wheel};
use aethereal_window::Event;
use winit::dpi::PhysicalPosition;
use winit::event::MouseScrollDelta;
use winit::keyboard::{Key, KeyCode, NamedKey, NativeKeyCode, PhysicalKey, SmolStr};

#[test]
fn a_line_of_the_wheel_is_forty_logical_pixels_at_the_scale_and_negated() {
    // Catches a line taken in surface pixels rather than logical ones, which scrolls half as far on a screen of two
    // pixels to each, and a turn left signed as winit signs it, which scrolls up for a wheel turned toward the person.
    let turned = wheel((10.0, 20.0), MouseScrollDelta::LineDelta(0.5, -3.0), 2.0);

    assert_eq!(turned, Event::Wheel { x: 10.0, y: 20.0, delta_x: -40.0, delta_y: 240.0 });
}

#[test]
fn a_turn_in_pixels_is_kept_in_the_surfaces_pixels_and_negated() {
    // Catches a turn in pixels scaled again, as a line is, when winit gives it in the screen's pixels already.
    let turned = wheel((0.0, 0.0), MouseScrollDelta::PixelDelta(PhysicalPosition::new(-7.0, 12.0)), 2.0);

    assert_eq!(turned, Event::Wheel { x: 0.0, y: 0.0, delta_x: 7.0, delta_y: -12.0 });
}

#[test]
fn keys_are_named_as_a_page_names_them() {
    // Catches the keys winit names apart from the specification sent under winit's names, which a program written
    // against a page's would not match, and an unplaced key given a name of winit's own.
    assert_eq!(code(PhysicalKey::Code(KeyCode::KeyW)), "KeyW");
    assert_eq!(code(PhysicalKey::Code(KeyCode::ArrowLeft)), "ArrowLeft");
    assert_eq!(code(PhysicalKey::Code(KeyCode::SuperLeft)), "MetaLeft");
    assert_eq!(code(PhysicalKey::Code(KeyCode::SuperRight)), "MetaRight");
    assert_eq!(code(PhysicalKey::Unidentified(NativeKeyCode::Xkb(9999))), "Unidentified");

    assert_eq!(key(&Key::Character(SmolStr::new("w"))), "w");
    assert_eq!(key(&Key::Named(NamedKey::Shift)), "Shift");
    assert_eq!(key(&Key::Named(NamedKey::Super)), "Meta");
    assert_eq!(key(&Key::Dead(Some('`'))), "Dead");
}
