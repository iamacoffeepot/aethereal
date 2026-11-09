//! What winit's events read as in the window capability's vocabulary (ADR-0024): the keys by their W3C UI Events
//! names, the pointer's buttons, the wheel's turn in the window's surface pixels, signed as a page signs it, and the
//! text a key typed or an input method committed (ADR-0051).

#[cfg(test)]
mod tests;

use aethereal_window::{Button, Event};
use winit::event::Ime;
use winit::event::MouseButton;
use winit::event::MouseScrollDelta;
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

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
/// specification calls `Meta`, and `Space`, which types a space and so is named by it, `" "`, and `Dead` or
/// `Unidentified` as a page names them.
pub fn key(key: &Key) -> String {
    match key {
        Key::Character(text) => text.to_string(),
        Key::Named(NamedKey::Super) => String::from("Meta"),
        Key::Named(NamedKey::Space) => String::from(" "),
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

/// Whether a window's input method has a composition open, as winit last said: from a `Preedit` with text until a
/// `Commit`, a `Preedit` with none, or `Disabled`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Composition {
    /// None is open: a key's text is sent.
    Closed,
    /// One is open: a key's text is not sent, as what the input method composes is sent once it commits it, and a
    /// character is never sent twice.
    Open,
}

/// The text a pressed key typed, sent after its `KeyDown`: `text`, winit's text for the key, with its control
/// characters removed, as Backspace, Enter, Tab, Escape, and Delete are only keys. None when that leaves nothing; when
/// Ctrl or Super is held in `modifiers`, as a chord types nothing, which winit's text does not count, as it counts every
/// modifier but Ctrl; and while `composition` is open.
pub fn typed(text: Option<&str>, modifiers: ModifiersState, composition: Composition) -> Option<String> {
    let chord = modifiers.control_key() || modifiers.super_key();
    let composing = composition == Composition::Open;
    if chord || composing {
        return None;
    }

    printable(text?)
}

/// The composition `ime`, heard while `composition` stood, leaves, and the text it sends: a commit's, sent once, which
/// closes the composition. A `Preedit` with text opens one, and one with none closes it, as `Disabled` does; the
/// uncommitted text itself is not sent.
pub fn composed(composition: Composition, ime: Ime) -> (Composition, Option<String>) {
    match ime {
        Ime::Enabled => (composition, None),
        Ime::Preedit(text, _) if text.is_empty() => (Composition::Closed, None),
        Ime::Preedit(..) => (Composition::Open, None),
        Ime::Commit(text) => (Composition::Closed, printable(&text)),
        Ime::Disabled => (Composition::Closed, None),
    }
}

/// `text` with its control characters removed: none when that leaves nothing.
fn printable(text: &str) -> Option<String> {
    let kept = text.chars().filter(|character| !character.is_control()).collect::<String>();
    if kept.is_empty() {
        return None;
    }

    Some(kept)
}
