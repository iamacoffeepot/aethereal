//! What an actor asks the window root, to open or close a window while the program runs, and how it is answered
//! (ADR-0027).

use crate::event::Surface;
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
