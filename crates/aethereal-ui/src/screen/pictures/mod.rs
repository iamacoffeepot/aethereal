//! A screen's pictures (ADR-0056: Pictures): a texture the renderer makes of each picture its widgets show, one for
//! each picture whatever widgets show it, kept while any does.
//!
//! ```text
//! layout ──> the pictures its tree shows ──> one not kept ──> CreateTexture ──> made ──> laid out again, drawn with it
//!                                        └─> one being made ──> the layout waits, as for a font's metrics
//!        ──> drawn ──> a kept one no widget shows ──> released once the list sent no longer names it
//! ```

#[cfg(test)]
mod tests;

use super::orders::Orders;
use super::program::Whitening;
use super::{Fresh, Screen};
use crate::pixels::{PictureId, Pixels};
use crate::tree::Flat;
use crate::widgets::Picture;
use aethereal_actor::{Answered, Level};
use aethereal_data::Plain;
use aethereal_render::Release;
use aethereal_render::texture::{TextureId, Textured};

/// A picture's texture was asked for: the picture, and the number of the making.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.ui.picturing")]
pub struct Picturing {
    picture: PictureId,
    serial: u64,
}

/// A texture was asked for: the white pixel every solid quad samples, as the screen's program is made, or a picture's.
/// One context for both, as an actor answers one kind of answer with one kind of context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.ui.texturing")]
pub enum Texturing {
    White(Whitening),
    Picture(Picturing),
}

/// What a screen has made of the pictures it shows: each picture's texture, by its id, in order, and the textures of
/// those no longer shown, released once the list sent no longer names them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Plain)]
pub(super) struct Pictures {
    kept: Vec<Kept>,
    releasing: Vec<TextureId>,
}

/// One picture's texture, and where its making stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
struct Kept {
    picture: PictureId,
    stage: Stage,
}

/// Where the making of a picture's texture stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Stage {
    /// It is asked for, by the making numbered `serial`.
    Asked {
        serial: u64,
    },
    Made(TextureId),
    /// The renderer refused it, and that was logged: the picture draws the library's white pixel, tinted.
    Refused,
}

/// Whether a layout's pictures are all made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Making {
    Done,
    /// One is still being made: the layout draws nothing, and the next after it is made lays out again.
    Under,
}

impl Pictures {
    /// The pictures `flat` shows, each once, in the order of their ids.
    fn shown<'flat>(flat: &[Flat<'flat>]) -> Vec<&'flat Pixels> {
        let mut shown: Vec<&Pixels> = flat
            .iter()
            .filter_map(|node| node.node.widget().as_any().downcast_ref::<Picture>())
            .map(|picture| &picture.pixels)
            .collect();
        shown.sort_unstable_by_key(|pixels| pixels.id());
        shown.dedup_by_key(|pixels| pixels.id());

        shown
    }

    /// Asks for a texture of each picture `flat` shows that is not kept, each making numbered from `serial` on: whether
    /// any is still being made, and the ids of every picture shown, in order.
    pub(super) fn ask(&mut self, flat: &[Flat<'_>], serial: &mut u64, orders: &mut Orders) -> (Making, Vec<PictureId>) {
        let shown = Self::shown(flat);
        let mut making = Making::Done;
        for pixels in &shown {
            let picture = pixels.id();
            let stage = match self.kept.binary_search_by_key(&picture, |kept| kept.picture) {
                Ok(at) => self.kept[at].stage,
                Err(at) => {
                    let asked = Stage::Asked { serial: *serial };
                    orders.ask_picture(Picturing { picture, serial: *serial }, (*pixels).clone());
                    *serial += 1;
                    self.kept.insert(at, Kept { picture, stage: asked });
                    asked
                }
            };
            if let Stage::Asked { .. } = stage {
                making = Making::Under;
            }
        }

        (making, shown.iter().map(|pixels| pixels.id()).collect())
    }

    /// Lets go of each picture kept that `shown`, every picture a layout about to be drawn shows, leaves out: its
    /// texture is released once the list that layout sends no longer names it.
    pub(super) fn unshown(&mut self, shown: &[PictureId]) {
        let Self { kept, releasing } = self;
        kept.retain(|kept| {
            if shown.binary_search(&kept.picture).is_ok() {
                return true;
            }
            if let Stage::Made(texture) = kept.stage {
                releasing.push(texture);
            }
            false
        });
    }

    /// Releases the textures of the pictures no longer shown: called once the list sent no longer names them.
    pub(super) fn release(&mut self, orders: &mut Orders) {
        for texture in self.releasing.drain(..) {
            orders.release(Release::Texture(texture));
        }
    }

    /// The texture made of `picture`, if one is.
    pub(super) fn texture(&self, picture: PictureId) -> Option<TextureId> {
        let at = self.kept.binary_search_by_key(&picture, |kept| kept.picture).ok()?;

        match self.kept[at].stage {
            Stage::Made(texture) => Some(texture),
            Stage::Asked { .. } | Stage::Refused => None,
        }
    }

    /// The picture kept as `picture`, if its making numbered `serial` is still under way.
    fn making(&mut self, picture: PictureId, serial: u64) -> Option<&mut Kept> {
        let at = self.kept.binary_search_by_key(&picture, |kept| kept.picture).ok()?;
        let kept = &mut self.kept[at];

        (kept.stage == Stage::Asked { serial }).then_some(kept)
    }
}

impl Screen {
    /// A texture was made or refused: the white pixel, or a picture's.
    pub fn textured(&mut self, answered: Answered<Textured>, context: Texturing) -> Orders {
        match context {
            Texturing::White(whitening) => self.whitened(answered, whitening),
            Texturing::Picture(picturing) => self.pictured(answered, picturing),
        }
    }

    /// A picture's texture was made or refused: made, it is kept and the next `Prepare` lays the screen out with it;
    /// refused, that is logged and the picture draws without it; one no longer shown is released.
    pub fn pictured(&mut self, answered: Answered<Textured>, context: Picturing) -> Orders {
        let mut orders = Orders::default();
        let Picturing { picture, serial } = context;
        let Some(kept) = self.pictures.making(picture, serial) else {
            if let Answered::Yes(Textured::Made(texture)) = answered {
                orders.release(Release::Texture(texture));
            }
            return orders;
        };
        kept.stage = match answered {
            Answered::Yes(Textured::Made(texture)) => Stage::Made(texture),
            refused => {
                orders.log(Level::Error, format!("the texture of the picture {picture:?} was not made: {refused:?}"));
                Stage::Refused
            }
        };
        self.fresh = Fresh::Stale;

        orders
    }
}
