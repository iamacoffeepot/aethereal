//! The cursors a host made of images: each distinct image made once, and the same one shown each time it is asked for
//! again (ADR-0055).

use super::CursorImage;
use aethereal_actor::BlobHash;
use aethereal_substrate::Engine;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

/// The cursors a host made of images, `C` its own kind of cursor, each kept under its image: the content hash of its
/// pixels, its size, and its hotspot. So an image shown again, or put again as another blob of the same bytes, is
/// made into a cursor once, and a host's cursors grow only with the distinct images shown, kept for as long as it
/// runs.
pub struct MadeCursors<C> {
    made: HashMap<Made, C>,
}

/// An image as a made cursor is kept under it: its pixels by their hash, its size, and its hotspot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Made {
    pixels: BlobHash,
    size: [u16; 2],
    hotspot: [u16; 2],
}

/// Why no cursor was made of an image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CursorUnmade<E> {
    /// The engine holds no blob its pixels refer to, as one of another engine's.
    Unheld,
    /// The host would not make it, as it said.
    Refused(E),
}

/// None made.
impl<C> Default for MadeCursors<C> {
    fn default() -> Self {
        Self { made: HashMap::new() }
    }
}

impl<C> MadeCursors<C> {
    /// The cursor made of `image`, whose pixels `engine` holds, between steps: made by `make` of the image and its
    /// pixels the first time an image of the same pixels, size, and hotspot is asked for, and the one made then each
    /// time after. One `make` refused is not kept, so the next ask makes it again.
    ///
    /// # Errors
    ///
    /// [`CursorUnmade::Unheld`] when `engine` holds no blob the pixels refer to, and [`CursorUnmade::Refused`] with
    /// why `make` would not make it.
    pub fn of<E>(
        &mut self,
        engine: &mut Engine,
        image: &CursorImage,
        make: impl FnOnce(&CursorImage, &[u8]) -> Result<C, E>,
    ) -> Result<&C, CursorUnmade<E>> {
        let pixels = engine.hash(&image.pixels).ok_or(CursorUnmade::Unheld)?;
        let key = Made { pixels, size: [image.width, image.height], hotspot: [image.hotspot_x, image.hotspot_y] };
        let vacant = match self.made.entry(key) {
            Entry::Occupied(kept) => return Ok(kept.into_mut()),
            Entry::Vacant(vacant) => vacant,
        };
        let bytes = engine.blob(&image.pixels).ok_or(CursorUnmade::Unheld)?;
        let cursor = make(image, bytes).map_err(CursorUnmade::Refused)?;

        Ok(vacant.insert(cursor))
    }
}
