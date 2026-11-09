//! The fonts the render root keeps: its own two, compiled in, each actor's it made, and the one list of fallbacks
//! (ADR-0052). A font is read whole as it is made and shared by every text laid out in it, so a text keeps drawing,
//! and is written, after its font is released.

use super::{FallbackFonts, Font};
use crate::font::{self, FontRefused};
use crate::made::Made;
use std::sync::Arc;

/// The renderer's own proportional font: Noto Sans Regular (SIL OFL), until faces are chosen (ADR-0052).
pub const SANS: &[u8] = include_bytes!("../../fonts/NotoSans-Regular.ttf");

/// The renderer's own monospace font: Noto Sans Mono Regular (SIL OFL), until faces are chosen (ADR-0052).
pub const MONO: &[u8] = include_bytes!("../../fonts/NotoSansMono-Regular.ttf");

/// Which font a glyph is of, as the atlas keys it: one of the renderer's own, or the one made under an id, which no
/// other font is given again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FontKey {
    Sans,
    Mono,
    Made(u64),
}

/// A font read, beside its key.
#[derive(Debug, Clone)]
pub struct Face {
    pub key: FontKey,
    pub font: Arc<font::Font>,
}

/// The fonts kept: the renderer's own two, those actors made, by id, and the fallbacks, the ids of the last list set.
pub struct Faces {
    sans: Arc<font::Font>,
    mono: Arc<font::Font>,
    pub made: Made<Arc<font::Font>>,
    fallbacks: Vec<u64>,
}

impl Faces {
    /// The renderer's own two fonts, read, and no other.
    ///
    /// # Errors
    ///
    /// Why one of its own was not read, which a font compiled in never is.
    pub fn new() -> Result<Self, FontRefused> {
        Ok(Self {
            sans: Arc::new(font::Font::read(SANS)?),
            mono: Arc::new(font::Font::read(MONO)?),
            made: Made::default(),
            fallbacks: Vec::new(),
        })
    }

    /// The font `font` names: none when it names one not made, or released.
    pub fn face(&self, font: Font) -> Option<Face> {
        let (key, font) = match font {
            Font::Sans => (FontKey::Sans, Arc::clone(&self.sans)),
            Font::Mono => (FontKey::Mono, Arc::clone(&self.mono)),
            Font::Made(id) => (FontKey::Made(id.id), Arc::clone(self.made.get(id.id)?)),
        };

        Some(Face { key, font })
    }

    /// Keeps `fallbacks` in place of the list before.
    pub fn fall_back(&mut self, fallbacks: &FallbackFonts) {
        self.fallbacks = fallbacks.fonts.iter().map(|font| font.id).collect();
    }

    /// The fallbacks still kept, in their list's order.
    pub fn fallbacks(&self) -> Vec<Face> {
        self.fallbacks
            .iter()
            .filter_map(|id| Some(Face { key: FontKey::Made(*id), font: Arc::clone(self.made.get(*id)?) }))
            .collect()
    }
}
