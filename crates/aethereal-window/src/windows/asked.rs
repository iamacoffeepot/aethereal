//! What an actor asks the window root, to open or close a window or set the mode it is shown in while the program
//! runs, and how it is answered (ADR-0027).

use crate::event::{Mode, Surface};
use crate::window::WindowId;
use aethereal_data::Plain;

/// A size in the host's logical pixels: the size a person sees whatever the screen, which its scale turns into the
/// surface's pixels once the window is on one.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
pub struct Logical {
    pub width: f64,
    pub height: f64,
}

/// Asks the window root for a window titled `title`, opened at `size` in the host's logical pixels: answered with
/// [`Opened`] once the host has made it, or would not.
#[derive(Debug, Clone, PartialEq, Plain)]
#[kind(name = "aether.windows.open_window")]
pub struct OpenWindow {
    pub title: String,
    pub size: Logical,
}

/// What became of a window asked for.
#[derive(Debug, Clone, PartialEq, Plain)]
#[kind(name = "aether.windows.opened")]
pub enum Opened {
    /// It is open, as `window`, its actor reached at `path::<Window>(window)`, its surface as it was opened: its size
    /// in its pixels, and its scale.
    Window { window: WindowId, surface: Surface },
    /// It was not opened, for this reason.
    Refused(Unopened),
}

/// Why a window asked for was not opened.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub enum Unopened {
    /// The host opens no window on request, as a page does not: it owns its canvases.
    NoHost,
    /// The windowing library or the GPU would not make the window or its surface, as the host says here.
    Failed(String),
    /// The host made it, and the actor of the window was refused: the host is asked to close it.
    Refused,
}

/// Asks the window root to close the window `window`: answered with [`Closed`] once the host has, or would not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.windows.close_window")]
pub struct CloseWindow {
    pub window: WindowId,
}

/// What became of a window asked to close.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.windows.closed")]
pub enum Closed {
    /// It is closed: its actor has sent its subscribers `Closed`, and ended.
    Closed,
    /// It was not closed, for this reason.
    Refused(Unclosed),
}

/// Why a window asked to close was not closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Unclosed {
    /// The window is not open: it never was, it is still opening, or it has closed.
    NotOpen,
    /// The host closes no window on request, as a page does not.
    NoHost,
}

/// Asks the window root to show the window `window` in `mode`: answered with [`ModeSet`] once the host has, or would
/// not. A window shown in `mode` already is answered at once, and its subscribers hear nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.windows.set_mode")]
pub struct SetMode {
    pub window: WindowId,
    pub mode: Mode,
}

/// What became of a window asked to be shown in a mode.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
#[kind(name = "aether.windows.mode_set")]
pub enum ModeSet {
    /// It is shown in the mode asked: this is its surface as the host draws it now, so the asker draws at it without
    /// waiting. Its subscribers hear `Moded` when the mode changed, and `Resized` when the surface did, as for any
    /// resize.
    Set { surface: Surface },
    /// Its mode was not set, for this reason.
    Refused(Unset),
}

/// Why a window asked to be shown in a mode was not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Unset {
    /// The window is not open: it never was, it is still opening, it is closing, or it closed before its mode was set.
    NotOpen,
    /// The host sets no window's mode on request, as a page does not: a page enters fullscreen only while it handles a
    /// person's click or key.
    NoHost,
}
