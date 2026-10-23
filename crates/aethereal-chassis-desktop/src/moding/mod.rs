//! A window's mode as winit shows it (ADR-0027): the attributes a window is opened with in a mode, a mode set on a
//! window shown, and the mode read back from what winit says of it, which the person at it may change.

#[cfg(test)]
mod tests;

use aethereal_window::Mode;
use winit::window::{Fullscreen, Window, WindowAttributes};

/// The attributes of a window, `attributes`, opened shown in `mode`: fullscreen borderless on the monitor it opens on,
/// or maximized.
pub fn opened_in(attributes: WindowAttributes, mode: Mode) -> WindowAttributes {
    let maximized = mode == Mode::Maximized;

    attributes.with_fullscreen(fullscreen(mode)).with_maximized(maximized)
}

/// Shows `window` in `mode`: out of fullscreen before it is maximized or not, so leaving fullscreen for `Windowed`
/// gives back the size it had.
pub fn show(window: &Window, mode: Mode) {
    window.set_fullscreen(fullscreen(mode));
    match mode {
        Mode::Windowed => window.set_maximized(false),
        Mode::Maximized => window.set_maximized(true),
        Mode::Fullscreen => {}
    }
}

/// The mode `window` is shown in, as winit says now.
pub fn shown(window: &Window) -> Mode {
    let fullscreen = window.fullscreen().is_some();

    read(fullscreen, window.is_maximized())
}

/// The mode of a window winit says is `fullscreen` or not, and `maximized` or not: fullscreen whether or not it is
/// maximized beneath, as a window maximized, then made fullscreen, is on some platforms.
pub const fn read(fullscreen: bool, maximized: bool) -> Mode {
    match (fullscreen, maximized) {
        (true, _) => Mode::Fullscreen,
        (false, true) => Mode::Maximized,
        (false, false) => Mode::Windowed,
    }
}

/// winit's fullscreen of `mode`: borderless, on the monitor the window is on, or none.
const fn fullscreen(mode: Mode) -> Option<Fullscreen> {
    match mode {
        Mode::Fullscreen => Some(Fullscreen::Borderless(None)),
        Mode::Windowed | Mode::Maximized => None,
    }
}
