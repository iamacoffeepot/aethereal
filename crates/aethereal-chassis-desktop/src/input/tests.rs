//! The translation's tests: winit's keys and wheel read as the window capability names and signs them, and the text a
//! key typed or an input method committed sent once, as the window capability sends text.

use super::{Composition, code, composed, key, typed, wheel};
use aethereal_window::Event;
use winit::dpi::PhysicalPosition;
use winit::event::{Ime, MouseScrollDelta};
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, NativeKeyCode, PhysicalKey, SmolStr};

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
    // against a page's would not match, a space named `Space` where a page gives the space it types, which leaves a
    // text field reading keys unable to type one, and an unplaced key given a name of winit's own.
    assert_eq!(code(PhysicalKey::Code(KeyCode::KeyW)), "KeyW");
    assert_eq!(code(PhysicalKey::Code(KeyCode::ArrowLeft)), "ArrowLeft");
    assert_eq!(code(PhysicalKey::Code(KeyCode::SuperLeft)), "MetaLeft");
    assert_eq!(code(PhysicalKey::Code(KeyCode::SuperRight)), "MetaRight");
    assert_eq!(code(PhysicalKey::Unidentified(NativeKeyCode::Xkb(9999))), "Unidentified");

    assert_eq!(key(&Key::Character(SmolStr::new("w"))), "w");
    assert_eq!(key(&Key::Named(NamedKey::Shift)), "Shift");
    assert_eq!(key(&Key::Named(NamedKey::Super)), "Meta");
    assert_eq!(key(&Key::Named(NamedKey::Space)), " ");
    assert_eq!(key(&Key::Dead(Some('`'))), "Dead");
}

#[test]
fn a_key_types_its_text_without_control_characters_and_never_in_a_chord() {
    // Catches the control character winit gives Backspace, Enter, or Tab sent as text, so a text field inserts what
    // the key already said; text left empty sent; a chord such as Ctrl+C or Super+C typing the `c` winit gives it; and
    // Alt or Shift, which type `é` or `E` on many layouts, suppressing the text they typed.
    let none = ModifiersState::empty();
    let closed = Composition::Closed;

    assert_eq!(typed(Some("é"), none, closed), Some(String::from("é")));
    assert_eq!(typed(Some("\u{8}"), none, closed), None);
    assert_eq!(typed(Some("\r"), none, closed), None);
    assert_eq!(typed(Some("a\u{7f}b"), none, closed), Some(String::from("ab")));
    assert_eq!(typed(None, none, closed), None);
    assert_eq!(typed(Some("c"), ModifiersState::CONTROL, closed), None);
    assert_eq!(typed(Some("c"), ModifiersState::SUPER, closed), None);
    assert_eq!(typed(Some("é"), ModifiersState::ALT, closed), Some(String::from("é")));
    assert_eq!(typed(Some("E"), ModifiersState::SHIFT, closed), Some(String::from("E")));
}

#[test]
fn a_composition_holds_back_key_text_and_sends_its_commit_once() {
    // Catches key text sent while an input method composes, which sends each character again with the commit; a
    // commit not sent, or one that leaves the composition open, so every key typed after it is dropped; and a
    // composition an empty preedit or a disabled input method ended left open, which drops keys' text for good.
    let none = ModifiersState::empty();
    let preedit = |text: &str| Ime::Preedit(String::from(text), None);

    let (open, sent) = composed(Composition::Closed, preedit("にほ"));
    assert_eq!((open, sent), (Composition::Open, None));
    assert_eq!(typed(Some("n"), none, open), None);
    assert_eq!(composed(open, Ime::Enabled), (Composition::Open, None));

    let (closed, sent) = composed(open, Ime::Commit(String::from("日本")));
    assert_eq!((closed, sent), (Composition::Closed, Some(String::from("日本"))));
    assert_eq!(typed(Some("w"), none, closed), Some(String::from("w")));

    assert_eq!(composed(open, preedit("")), (Composition::Closed, None));
    assert_eq!(composed(open, Ime::Disabled), (Composition::Closed, None));
}
