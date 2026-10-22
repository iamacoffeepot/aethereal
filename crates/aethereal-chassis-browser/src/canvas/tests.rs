//! The canvas's tests: what the page's pointer and wheel events read as, in the canvas's pixels and its buttons, and
//! how many pixels the canvas is drawn at.

use super::dom::{Canvas, Density, Measured, Pointing, button_of, css_pixels};
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

#[test]
fn a_canvas_is_drawn_at_the_screens_pixels_of_the_size_it_is_laid_out_at() {
    // Catches CSS pixels left unscaled by the screen's ratio, device pixels scaled by it a second time, a side rounded
    // down rather than to the nearest pixel, and the axes swapped.
    let css = Measured::CssPixels { width: 400.4, height: 300.6 };
    let device = Measured::DevicePixels { width: 801.0, height: 601.0 };

    assert_eq!(css.pixels(Density::SCREEN, 2.0, 8192), (801, 601));
    assert_eq!(device.pixels(Density::SCREEN, 2.0, 8192), (801, 601));
    assert_eq!(css.pixels(Density::SCREEN, 1.0, 8192), (400, 301));
}

#[test]
fn a_density_draws_a_canvas_at_no_more_pixels_than_it_allows() {
    // Catches the most taken as the least, a screen of fewer pixels than the most drawn at the most, device pixels
    // left at the screen's ratio past the most, and a most of no pixels or no number drawing nothing.
    let at_most_two = Density::at_most(2.0);
    let css = Measured::CssPixels { width: 400.0, height: 300.0 };
    let device = Measured::DevicePixels { width: 1200.0, height: 900.0 };

    assert_eq!(css.pixels(at_most_two, 3.0, 8192), (800, 600));
    assert_eq!(device.pixels(at_most_two, 3.0, 8192), (800, 600));
    assert_eq!(css.pixels(at_most_two, 1.5, 8192), (600, 450));
    assert_eq!(css.pixels(Density::at_most(0.5), 1.0, 8192), (200, 150));
    assert_eq!(css.pixels(Density::at_most(0.0), 3.0, 8192), (1200, 900));
    assert_eq!(css.pixels(Density::at_most(f64::NAN), 3.0, 8192), (1200, 900));
}

#[test]
fn a_canvas_laid_out_at_no_size_or_past_the_longest_side_is_drawn_at_a_size_the_gpu_makes() {
    // Catches a canvas laid out at no pixels drawn at none, which no surface is configured to, a side past the GPU's
    // longest left past it, and a screen that gives no ratio drawing at none.
    let hidden = Measured::CssPixels { width: 0.0, height: 0.3 };
    let wide = Measured::DevicePixels { width: 20_000.0, height: 100.0 };

    assert_eq!(hidden.pixels(Density::SCREEN, 2.0, 8192), (1, 1));
    assert_eq!(wide.pixels(Density::SCREEN, 2.0, 8192), (8192, 100));
    assert_eq!(Measured::CssPixels { width: 40.0, height: 30.0 }.pixels(Density::SCREEN, 0.0, 8192), (40, 30));
}
