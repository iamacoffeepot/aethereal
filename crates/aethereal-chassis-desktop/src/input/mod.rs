//! What winit's events read as in the window capability's vocabulary (ADR-0024): the keys by their W3C UI Events
//! names, the pointer's buttons, and the wheel's turn in the window's surface pixels, signed as a page signs it.

#[cfg(test)]
mod tests;

use aethereal_window::{Button, Event};
use winit::event::MouseButton;
use winit::event::MouseScrollDelta;
use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};

/// The logical pixels a line of the wheel's turn is taken as, for a host that counts the wheel in lines (ADR-0024).
pub const LINE_LOGICAL_PIXELS: f64 = 40.0;

/// The W3C UI Events `code` of the key at `key`: the name of its `KeyCode` variant, which winit names after the
/// specification, but for the two keys winit names otherwise, `SuperLeft` and `SuperRight`, which the specification
/// calls `MetaLeft` and `MetaRight`. A key winit could not place is `Unidentified`, as a page names one.
pub fn code(key: PhysicalKey) -> String {
    match key {
        PhysicalKey::Code(KeyCode::SuperLeft) => String::from("MetaLeft"),
        PhysicalKey::Code(KeyCode::SuperRight) => String::from("MetaRight"),
        PhysicalKey::Code(code) => format!("{code:?}"),
        PhysicalKey::Unidentified(_) => String::from("Unidentified"),
    }
}

/// The W3C UI Events `key` of `key`, what it means under the layout: the text of a key that types it, the name of a
/// named key's `NamedKey` variant, which winit names after the specification, but for `Super`, which the
/// specification calls `Meta`, and `Dead` or `Unidentified` as a page names them.
pub fn key(key: &Key) -> String {
    match key {
        Key::Character(text) => text.to_string(),
        Key::Named(NamedKey::Super) => String::from("Meta"),
        Key::Named(named) => format!("{named:?}"),
        Key::Dead(_) => String::from("Dead"),
        Key::Unidentified(_) => String::from("Unidentified"),
    }
}

/// The pointer's button winit names `button`.
pub const fn button(button: MouseButton) -> Button {
    match button {
        MouseButton::Left => Button::Main,
        MouseButton::Right => Button::Secondary,
        MouseButton::Middle => Button::Middle,
        MouseButton::Back => Button::Back,
        MouseButton::Forward => Button::Forward,
        MouseButton::Other(number) => Button::Other(number),
    }
}

/// The event of the wheel turned by `delta`, with the pointer at `x`, `y` in the window's surface pixels, on a screen
/// of `scale` of them to each logical pixel. A turn in pixels is in the screen's, the surface's, as it is; one in lines
/// is [`LINE_LOGICAL_PIXELS`] logical pixels a line, times `scale`. Each is negated: winit signs a delta by where the
/// content moves, and the window capability by where the view scrolls, positive down and right.
pub fn wheel((x, y): (f32, f32), delta: MouseScrollDelta, scale: f64) -> Event {
    let (across, down) = match delta {
        MouseScrollDelta::PixelDelta(moved) => (moved.x, moved.y),
        MouseScrollDelta::LineDelta(across, down) => {
            let per = LINE_LOGICAL_PIXELS * scale;
            (f64::from(across) * per, f64::from(down) * per)
        }
    };
    // A turn is some thousands of pixels at most, which an f32 holds to a small fraction of one.
    #[allow(clippy::cast_possible_truncation)]
    let (delta_x, delta_y) = (-across as f32, -down as f32);

    Event::Wheel { x, y, delta_x, delta_y }
}
