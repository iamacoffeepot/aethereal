//! The canvas's tests: what the page's pointer events read as, in the canvas's pixels and its buttons.

use super::dom::{Canvas, Pointing, button_of};
use aethereal_window::{Button, Event};

#[test]
fn a_point_on_the_canvas_is_scaled_to_its_backing_store_on_each_axis() {
    // Catches a scale taken the wrong way up (shown over stored), one axis scaled by the other's ratio, and a canvas
    // shown with no size giving a point no number.
    let wide = Canvas::new(1600, 900, 800.0, 600.0);
    let hidden = Canvas::new(300, 150, 0.0, 0.0);

    assert_eq!(wide.pixel(400.0, 300.0), (800.0, 450.0));
    assert_eq!(wide.pixel(0.0, 0.0), (0.0, 0.0));
    assert_eq!(hidden.pixel(10.0, 20.0), (10.0, 20.0));
}

#[test]
fn the_pages_button_numbers_are_its_buttons_and_a_move_carries_none() {
    // Catches the middle and right buttons swapped, as the page numbers the wheel 1 and the right button 2, a number
    // past the five named taken for one of them, a press of no button (the page's -1) read as one, and a press or
    // release that reads no button.
    let named = [0, 1, 2, 3, 4, 7, -1].map(button_of);
    let at = (3.0, 4.0);

    let expected = [
        Some(Button::Main),
        Some(Button::Middle),
        Some(Button::Secondary),
        Some(Button::Back),
        Some(Button::Forward),
        Some(Button::Other(7)),
        None,
    ];
    assert_eq!(named, expected);
    assert_eq!(Pointing::Moved.event(at, -1), Some(Event::PointerMoved { x: 3.0, y: 4.0 }));
    assert_eq!(Pointing::Down.event(at, 2), Some(Event::PointerDown { x: 3.0, y: 4.0, button: Button::Secondary }));
    assert_eq!(Pointing::Up.event(at, 0), Some(Event::PointerUp { x: 3.0, y: 4.0, button: Button::Main }));
}
