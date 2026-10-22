//! The canvas's tests: what the page's pointer and wheel events read as, in the canvas's pixels and its buttons.

use super::dom::{Canvas, Pointing, button_of, css_pixels};
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

#[test]
fn a_wheel_counted_in_lines_or_pages_is_taken_in_pixels_by_its_fixed_factor() {
    // Catches a line and a page taken by each other's factor, a mode's numbers shifted by one, a delta in pixels
    // scaled at all, a sign flipped, and a mode the page has no name for read as one it has.
    let turned = [0, 1, 2, 3].map(|mode| css_pixels(-3.0, mode));

    assert_eq!(turned, [Some(-3.0), Some(-120.0), Some(-2400.0), None]);
}

#[test]
fn a_wheel_over_the_canvas_reads_its_place_and_its_turn_in_the_canvas_pixels() {
    // Catches the turn left in CSS pixels on a canvas shown smaller than its store, the axes of the place or of the
    // turn swapped, the place left unscaled, and a turn in an unnamed mode pushed.
    let small = Canvas::new(800, 600, 400.0, 150.0);

    let lines = small.wheel((100.0, 50.0), (1.0, -2.0), 1);
    assert_eq!(lines, Some(Event::Wheel { x: 200.0, y: 200.0, delta_x: 80.0, delta_y: -320.0 }));
    assert_eq!(
        small.wheel((0.0, 0.0), (0.0, 100.0), 0),
        Some(Event::Wheel { x: 0.0, y: 0.0, delta_x: 0.0, delta_y: 400.0 })
    );
    assert_eq!(small.wheel((0.0, 0.0), (0.0, 1.0), 7), None);
}
