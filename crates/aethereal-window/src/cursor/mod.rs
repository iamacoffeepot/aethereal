//! What a window shows at the pointer (ADR-0055): the cursor is declared where drawing is. Each draw-list entry the
//! pointer can reach carries a [`Cursor`], and the window shows that of the topmost entry the pointer reaches at its
//! place, as its cascade finds it, or, while a button is held, that of the topmost entry the press reached, wherever
//! the pointer goes. Over nothing the pointer reaches, it shows the default.
//!
//! A window's actor asks its root to show a cursor, [`ShowCursor`], only as the one it shows changes, and the root asks
//! its host. A cursor made of an image is the image's pixels as a blob (ADR-0014, ADR-0042), with its size and hotspot;
//! a host makes each distinct image into a cursor of its own once, and reuses it, [`MadeCursors`].

mod made;
#[cfg(test)]
mod tests;

pub use made::{CursorUnmade, MadeCursors};

use crate::window::WindowId;
use aethereal_actor::Bytes;
use aethereal_data::{Bound, Plain};

/// What a window shows at the pointer: the host's own cursor, none, one of the host's icons, or an image.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.window.cursor")]
pub enum Cursor {
    /// The host's own cursor, as it shows where nothing says otherwise: an arrow.
    Default,
    /// No cursor: the pointer is still there and still reaches what it is over.
    Hidden,
    /// The text cursor, an I-beam: over text that can be selected or typed into.
    Text,
    /// A hand, pointing: over something that acts when pressed, as a link does.
    Hand,
    /// An open hand: over something that can be dragged.
    Grab,
    /// A closed hand: dragging.
    Grabbing,
    /// A circle struck through: what is pointed at cannot be pressed, or a drop there is refused.
    NotAllowed,
    /// An image, as an item being dragged shows itself.
    Image(CursorImage),
}

/// A cursor's image: `width` by `height` pixels, each four bytes, red, green, blue, and alpha, not premultiplied, the
/// rows from the top, held as a blob, too large for a piece of mail; and its hotspot, the pixel that points, from its
/// top left. A side is at least one pixel and at most [`MOST_CURSOR_SIDE`], and the hotspot is within the image.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.window.cursor_image")]
pub struct CursorImage {
    pub pixels: Bytes,
    pub width: u16,
    pub height: u16,
    pub hotspot_x: u16,
    pub hotspot_y: u16,
}

/// The longest side of a cursor's image, in its pixels: the most a browser shows as a cursor, as it ignores a larger
/// one; a native host shows larger.
pub const MOST_CURSOR_SIDE: u16 = 128;

/// Why a cursor's image cannot be shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum CursorUnfit {
    /// A side is no pixels, or longer than [`MOST_CURSOR_SIDE`].
    Size,
    /// The hotspot lies outside the image.
    Hotspot,
    /// Its pixels are not four bytes for each pixel of its size.
    Pixels,
}

impl CursorImage {
    /// Checks the image against its pixels' length, `bytes`, which only a turn reads: refused when a side is no pixels
    /// or too long, the hotspot lies outside it, or the pixels are not four bytes for each of its pixels.
    ///
    /// # Errors
    ///
    /// Why it cannot be shown.
    pub fn check(&self, bytes: usize) -> Result<(), CursorUnfit> {
        let sides = [self.width, self.height];
        let sized = sides.iter().all(|side| (1..=MOST_CURSOR_SIDE).contains(side));
        if !sized {
            return Err(CursorUnfit::Size);
        }
        let within_across = self.hotspot_x < self.width;
        let within_down = self.hotspot_y < self.height;
        let within = within_across && within_down;
        if !within {
            return Err(CursorUnfit::Hotspot);
        }
        let pixels = usize::from(self.width) * usize::from(self.height);
        if bytes != pixels * 4 {
            return Err(CursorUnfit::Pixels);
        }

        Ok(())
    }
}

/// Asks the window root to have its host show `cursor` at the pointer over `window`: sent by the window's own actor,
/// as the cursor it shows changes, and acted on only from it. It has no answer: the next change replaces it.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.windows.show_cursor")]
pub struct ShowCursor {
    pub window: WindowId,
    pub cursor: Cursor,
}
