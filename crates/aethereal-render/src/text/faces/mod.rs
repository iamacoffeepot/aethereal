//! The fonts the render root keeps (ADR-0052): its own two, compiled in, and each an actor made, each a face of its own
//! and the faces of its fallbacks, fixed as it was made. A face is read once and shared, by every font made from it and
//! every text laid out in it, so a text keeps drawing, and is written, after its font is released, and a font keeps its
//! fallbacks after they are released. Each face's metrics are encoded once, as a blob kept for the face's life, and
//! every ask for a font's metrics is answered with those blobs.

#[cfg(test)]
pub mod tests;

use super::layout::{self, Laid, LaidGlyph, Which};
use super::{Align, Font, FontId, FontMetrics, FontNotMade, Run, Wrap};
use crate::font::{self, Metrics};
use crate::made::Made;
use crate::text::atlas::GlyphKey;
use aethereal_actor::{Bytes, Refused};
use std::sync::Arc;

/// The renderer's own proportional font: IBM Plex Sans Regular (SIL OFL), as the owner chose (ADR-0052).
pub const SANS: &[u8] = include_bytes!("../../../fonts/IBMPlexSans-Regular.ttf");

/// The renderer's own monospace font: IBM Plex Mono Regular (SIL OFL), as the owner chose (ADR-0052).
pub const MONO: &[u8] = include_bytes!("../../../fonts/IBMPlexMono-Regular.ttf");

/// Which font a glyph is of, as the atlas keys it: one of the renderer's own, or one read from bytes, numbered as it
/// was read, a number no other is given again. A font made from one of the renderer's own is keyed as that one, so its
/// glyphs are the ones already in the atlas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FontKey {
    Sans,
    Mono,
    Read(u64),
}

/// A font read, beside its key and its metrics as the blob they were encoded into.
#[derive(Debug, Clone)]
pub struct Face {
    pub key: FontKey,
    pub font: Arc<font::Font>,
    pub metrics: Bytes,
}

/// A font as texts are laid out in it: its own face, and its fallbacks' own faces, in the order they are tried. A
/// fallback's own fallbacks are not among them.
#[derive(Debug, Clone)]
pub struct Typeface {
    pub own: Face,
    pub fallbacks: Vec<Face>,
}

/// What a font is made of: one of the renderer's own, shared, or a font read from bytes, with its metrics put.
pub enum Own {
    Sans,
    Mono,
    Read { font: Arc<font::Font>, metrics: Bytes },
}

/// The fonts kept: the renderer's own two, each with no fallbacks, those actors made, by id, and how many fonts have
/// been read from bytes, which numbers the next.
pub struct Faces {
    sans: Typeface,
    mono: Typeface,
    pub made: Made<Typeface>,
    read: u64,
}

impl Face {
    /// The font `font` read, keyed `key`, its metrics put as a blob by `put`.
    ///
    /// # Errors
    ///
    /// [`FontNotMade::Unstored`] when the blob was not put.
    pub fn new(
        key: FontKey,
        font: Arc<font::Font>,
        put: impl FnOnce(Vec<u8>) -> Result<Bytes, Refused>,
    ) -> Result<Self, FontNotMade> {
        let metrics = put(font.metrics().encode()).map_err(|_| FontNotMade::Unstored)?;

        Ok(Self { key, font, metrics })
    }
}

/// A face's metrics are its font's.
impl AsRef<Metrics> for Face {
    fn as_ref(&self) -> &Metrics {
        self.font.metrics()
    }
}

impl Typeface {
    /// The face of the font `which` names: its own, or the fallback at its index. None for a fallback it has not.
    pub fn face(&self, which: Which) -> Option<&Face> {
        match which {
            Which::Own => Some(&self.own),
            Which::Fallback(at) => self.fallbacks.get(at),
        }
    }

    /// `runs` laid out in it at `size` pixels to the em, wrapped and aligned as `shaping` says.
    pub fn lay_out(&self, size: f32, runs: &[Run], shaping: (Wrap, Align)) -> Laid {
        layout::lay_out(self.own.font.metrics(), &self.fallbacks, size, runs, shaping)
    }

    /// The atlas's key for `glyph`, of a text laid out in it. None for a glyph of a font it has not.
    pub fn key(&self, glyph: &LaidGlyph) -> Option<GlyphKey> {
        self.face(glyph.font).map(|face| GlyphKey { font: face.key, glyph: glyph.glyph })
    }

    /// The atlas's keys for every glyph `laid`, a text laid out in it, draws, each once.
    pub fn keys(&self, laid: &Laid) -> impl Iterator<Item = GlyphKey> + use<> {
        let mut keys: Vec<GlyphKey> = laid.glyphs.iter().filter_map(|glyph| self.key(glyph)).collect();
        keys.sort_unstable();
        keys.dedup();

        keys.into_iter()
    }

    /// Its metrics, as the blobs kept for them: its own face's, then each fallback's, in order.
    pub fn metrics(&self) -> FontMetrics {
        let fallbacks = self.fallbacks.iter().map(|face| face.metrics.clone()).collect();

        FontMetrics::Read { own: self.own.metrics.clone(), fallbacks }
    }
}

impl Faces {
    /// The renderer's own two fonts, read, their metrics put as blobs by `put`, and no other.
    ///
    /// # Errors
    ///
    /// Why one of its own was not read, which a font compiled in never is, or its metrics not put.
    pub fn new(mut put: impl FnMut(Vec<u8>) -> Result<Bytes, Refused>) -> Result<Self, FontNotMade> {
        let mut own = |key, bytes| {
            let font = font::Font::read(bytes).map_err(FontNotMade::Read)?;
            let own = Face::new(key, Arc::new(font), &mut put)?;
            Ok::<_, FontNotMade>(Typeface { own, fallbacks: Vec::new() })
        };

        Ok(Self { sans: own(FontKey::Sans, SANS)?, mono: own(FontKey::Mono, MONO)?, made: Made::default(), read: 0 })
    }

    /// The font `font` names: none when it names one not made, or released.
    pub fn typeface(&self, font: Font) -> Option<&Typeface> {
        match font {
            Font::Sans => Some(&self.sans),
            Font::Mono => Some(&self.mono),
            Font::Made(id) => self.made.get(id.id),
        }
    }

    /// The font made of `own` with the fallbacks `fallbacks` name, to keep: the face of one of the renderer's own
    /// shared, its glyphs the ones in the atlas, and each fallback's own face, so it does not change when they are
    /// released, and their fallbacks are not tried.
    ///
    /// # Errors
    ///
    /// [`FontNotMade::Fallback`], naming the first fallback that is not kept: never made, or released.
    pub fn make(&mut self, own: Own, fallbacks: &[FontId]) -> Result<Typeface, FontNotMade> {
        let fallbacks = self.fallbacks(fallbacks)?;
        let own = match own {
            Own::Sans => self.sans.own.clone(),
            Own::Mono => self.mono.own.clone(),
            Own::Read { font, metrics } => {
                self.read += 1;
                Face { key: FontKey::Read(self.read), font, metrics }
            }
        };

        Ok(Typeface { own, fallbacks })
    }

    /// The fallbacks `fonts` name, each its own face, in order.
    fn fallbacks(&self, fonts: &[FontId]) -> Result<Vec<Face>, FontNotMade> {
        fonts
            .iter()
            .zip(0..)
            .map(|(font, fallback)| {
                let kept = self.made.get(font.id).ok_or(FontNotMade::Fallback { fallback })?;
                Ok(kept.own.clone())
            })
            .collect()
    }
}
