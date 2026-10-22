//! What a window hears, as its subscribers are sent it, and how they subscribe.

use aethereal_data::Plain;

/// What a window heard, or how it stands, sent to the actors subscribed to it in the order its host heard it.
///
/// A position is in the window's surface pixels: those of the surface the renderer draws in, from its top left.
#[derive(Debug, Clone, PartialEq, Plain)]
#[kind(name = "aether.window.event")]
pub enum Event {
    /// The pointer moved over the window, to `x`, `y`.
    PointerMoved { x: f32, y: f32 },
    /// A button of the pointer went down over the window, at `x`, `y`.
    PointerDown { x: f32, y: f32, button: Button },
    /// A button of the pointer came up over the window, at `x`, `y`.
    PointerUp { x: f32, y: f32, button: Button },
    /// The wheel turned with the pointer over the window at `x`, `y`, by `delta_x` across and `delta_y` down.
    ///
    /// Each delta is in the window's surface pixels, as a position is: how far the host would scroll what is under the
    /// pointer. A positive `delta_y` scrolls down, toward what is below, as a wheel turned toward the person at it
    /// does; a positive `delta_x` scrolls right. A host that counts the wheel in lines or pages takes a line as 40 of
    /// its logical pixels and a page as 800, and converts those to the surface's.
    Wheel { x: f32, y: f32, delta_x: f32, delta_y: f32 },
    /// A key went down while the window had the keyboard: `code` is the key's place on the keyboard, whatever its
    /// layout, as the W3C UI Events name it (`"KeyW"`, `"ArrowLeft"`), and `key` what it means under the layout
    /// (`"w"`, `"Shift"`). `repeat` when it is held down and this is the keyboard's repeat.
    KeyDown { code: String, key: String, repeat: bool },
    /// A key came up, named as when it went down.
    KeyUp { code: String, key: String },
    /// The window's surface is now this size, or its screen this scale; or this is how it stands, sent as the actor
    /// subscribed.
    Resized(Surface),
    /// The window has the keyboard; or it has it as the actor subscribed.
    FocusGained,
    /// The window no longer has the keyboard; or it has not as the actor subscribed.
    FocusLost,
    /// The window was asked to close, by the person at it. It stays open: what to do is the program's.
    CloseRequested,
}

/// A window's surface: what the renderer draws in, `width` by `height` of its pixels, on a screen of `scale` of them
/// to each of the host's logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
pub struct Surface {
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

/// A button of the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Button {
    /// The left button, or a touch or pen's contact.
    Main,
    /// The wheel, or the middle button.
    Middle,
    /// The right button.
    Secondary,
    /// The button that goes back.
    Back,
    /// The button that goes forward.
    Forward,
    /// Any other, by the host's number for it.
    Other(u16),
}

/// Asks a window to send this actor how it stands now, and every event from the next it hears: the sender must handle
/// [`Event`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.window.subscribe")]
pub struct Subscribe;

/// Asks a window to send this actor nothing more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.window.unsubscribe")]
pub struct Unsubscribe;
