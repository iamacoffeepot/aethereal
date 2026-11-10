//! The atlas (ADR-0052): where each glyph's distance field lies in the one texture every text is drawn from. Fields
//! are packed on shelves; when one does not fit, the atlas grows, doubling its side up to the GPU's longest; when it can
//! grow no further, it is packed again from nothing, the glyphs drawn most lately first, and the glyphs drawn longest
//! ago that no longer fit are dropped, to be generated again when a text needs them. This is its bookkeeping; the
//! texture is [`texture`]'s.

pub mod shelf;
#[cfg(test)]
mod tests;
pub mod texture;

use super::faces::FontKey;
use crate::font::GlyphId;
use shelf::Shelves;
use std::collections::HashMap;

/// A glyph of a font, as the atlas keys it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GlyphKey {
    pub font: FontKey,
    pub glyph: GlyphId,
}

/// A glyph's field as it was generated at the base size: its texels, and where its top left lies from the glyph's
/// origin, right and up, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub width: u32,
    pub height: u32,
    pub left: i32,
    pub top: i32,
}

impl Field {
    /// Whether it has no texels: a space's, which is never packed.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// Where a glyph's field lies in the atlas: its top left texel, and the field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub x: u32,
    pub y: u32,
    pub field: Field,
}

/// The texels left between two fields, so that filtering at one field's edge reads none of its neighbour's.
pub const GAP: u32 = 1;

/// The side the atlas is first made at, in texels.
pub const FIRST_SIDE: u32 = 512;

/// The glyphs in the atlas: where each lies, the shelves they were packed on, its side, the longest side it may grow
/// to, and how many times it has been packed again, which moves glyphs, so a text drawn from where they were is laid
/// into the atlas again.
#[derive(Debug)]
pub struct Atlas {
    side: u32,
    most: u32,
    shelves: Shelves,
    slots: HashMap<GlyphKey, Slot>,
    packings: u64,
}

/// How the texture is changed before the glyphs just added are written into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arranged {
    /// It is not: every glyph fitted where it was.
    Kept,
    /// It is made again at `side`, what it held copied to the same place.
    Grown { side: u32 },
    /// It is made again at `side`, holding only the glyphs kept, each copied as `moves` says.
    Packed { side: u32, moves: Vec<Move> },
}

/// A field copied from where it lay to where it lies now, in texels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Move {
    pub from: (u32, u32),
    pub to: (u32, u32),
    pub width: u32,
    pub height: u32,
}

/// What adding glyphs did: how the texture is changed, where each glyph that fitted lies, to write its field, and
/// those that fitted nowhere, even packed again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Added {
    pub arranged: Arranged,
    pub placed: Vec<(GlyphKey, Slot)>,
    pub unfit: Vec<GlyphKey>,
}

impl Atlas {
    /// An empty atlas of [`FIRST_SIDE`] texels a side, or `most` when that is less, which grows to at most `most`.
    #[must_use]
    pub fn new(most: u32) -> Self {
        let side = FIRST_SIDE.min(most);

        Self { side, most, shelves: Shelves::new(side), slots: HashMap::new(), packings: 0 }
    }

    /// Its side, in texels.
    #[must_use]
    pub const fn side(&self) -> u32 {
        self.side
    }

    /// The longest side it may grow to, in texels.
    #[must_use]
    pub const fn most(&self) -> u32 {
        self.most
    }

    /// The texels its glyphs' fields cover, without the gaps between them.
    #[must_use]
    pub fn used_texels(&self) -> u64 {
        self.slots.values().map(|slot| u64::from(slot.field.width) * u64::from(slot.field.height)).sum()
    }

    /// How many times it has been packed again: a text laid into it at another count is laid into it again.
    #[must_use]
    pub const fn packings(&self) -> u64 {
        self.packings
    }

    /// Where `key` lies: none when it is not in the atlas.
    #[must_use]
    pub fn slot(&self, key: GlyphKey) -> Option<Slot> {
        self.slots.get(&key).copied()
    }

    /// Whether every one of `keys` is in the atlas.
    pub fn holds(&self, mut keys: impl Iterator<Item = GlyphKey>) -> bool {
        keys.all(|key| self.slots.contains_key(&key))
    }

    /// Adds the glyphs `fields`, each beside its key: each packed where it fits, the atlas grown when one does not, and
    /// packed again from nothing when it can grow no further, keeping first the glyphs drawn at `latest`, then those
    /// added, then the rest, most lately drawn first, as `drawn` says when each glyph was last drawn. A glyph in the
    /// atlas already is left where it is.
    pub fn add(&mut self, fields: &[(GlyphKey, Field)], drawn: &HashMap<GlyphKey, u64>, latest: u64) -> Added {
        let first_side = self.side;
        let mut placed = Vec::with_capacity(fields.len());
        let mut fitted = true;
        for (key, field) in fields {
            if self.slots.contains_key(key) {
                continue;
            }
            let Some(slot) = self.pack(*field) else {
                fitted = false;
                break;
            };
            self.slots.insert(*key, slot);
            placed.push((*key, slot));
        }
        if fitted {
            let grown = self.side != first_side;
            let arranged = if grown {
                Arranged::Grown { side: self.side }
            } else {
                Arranged::Kept
            };
            return Added { arranged, placed, unfit: Vec::new() };
        }

        for (key, _) in &placed {
            self.slots.remove(key);
        }
        self.pack_again(fields, drawn, latest)
    }

    /// Where `field` lies once packed: on a shelf with room, the atlas grown until one has room. None when it fits
    /// nowhere at the longest side.
    fn pack(&mut self, field: Field) -> Option<Slot> {
        if field.is_empty() {
            return Some(Slot { x: 0, y: 0, field });
        }
        loop {
            if let Some((x, y)) = self.shelves.place(field.width + GAP, field.height + GAP) {
                return Some(Slot { x, y, field });
            }
            if self.side >= self.most {
                return None;
            }
            self.side = self.side.saturating_mul(2).min(self.most);
            self.shelves.grow(self.side);
        }
    }

    /// Packs the atlas again from nothing at its longest side, with the glyphs `fields` added, as [`add`](Self::add)
    /// says.
    fn pack_again(&mut self, fields: &[(GlyphKey, Field)], drawn: &HashMap<GlyphKey, u64>, latest: u64) -> Added {
        self.side = self.most;
        self.shelves = Shelves::new(self.side);
        self.packings += 1;
        let mut held: Vec<(GlyphKey, Slot)> = self.slots.drain().collect();
        let when = |key: &GlyphKey| drawn.get(key).copied().unwrap_or(0);
        // The most lately drawn first, and among those drawn together, in the keys' order, so packing is the same each
        // time.
        held.sort_unstable_by(|(left, _), (right, _)| when(right).cmp(&when(left)).then(left.cmp(right)));
        let (shown, rest): (Vec<_>, Vec<_>) = held.into_iter().partition(|(key, _)| when(key) == latest);

        let mut moves = Vec::new();
        for (key, slot) in shown {
            self.keep(key, slot, &mut moves);
        }
        let mut placed = Vec::new();
        let mut unfit = Vec::new();
        for (key, field) in fields {
            if self.slots.contains_key(key) {
                continue;
            }
            match self.pack(*field) {
                Some(slot) => {
                    self.slots.insert(*key, slot);
                    placed.push((*key, slot));
                }
                None => unfit.push(*key),
            }
        }
        for (key, slot) in rest {
            self.keep(key, slot, &mut moves);
        }

        Added { arranged: Arranged::Packed { side: self.side, moves }, placed, unfit }
    }

    /// Keeps `key`, which lay at `slot` before the atlas was packed again, where it fits now, noting its move in
    /// `moves`: dropped when it fits nowhere.
    fn keep(&mut self, key: GlyphKey, slot: Slot, moves: &mut Vec<Move>) {
        let Some(kept) = self.pack(slot.field) else {
            return;
        };
        self.slots.insert(key, kept);
        if !slot.field.is_empty() {
            let (width, height) = (slot.field.width, slot.field.height);
            moves.push(Move { from: (slot.x, slot.y), to: (kept.x, kept.y), width, height });
        }
    }
}
