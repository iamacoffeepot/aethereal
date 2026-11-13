//! Composites a screen's view keeps while their inputs stand (ADR-0056: A screen is an actor; Your own widgets): an
//! element marked `memo` in a screen's `view!` is built only when what its markup writes into it differs from what it
//! was last built from at its key path.
//!
//! A composite is a function of its params: its tree is built from them alone, with no state and no `self`. So a
//! composite placed at the same key path, with the same type, the same params, the same events, and the same style,
//! builds the same tree, and the tree it built the last time is taken in its place. What its markup fixes, a literal
//! and a field left at its default, is the same at every build of one place in the markup, which its key path names.
//!
//! ```text
//! view: memo element ──> Screen::memo ──> inputs kept at its path, of its type, equal? ──> Node::kept (a marker)
//!                                                              └─> otherwise: built, its inputs carried on its node
//! merge: marker ──> the last tree's node at its path, taken whole ──> compared, flattened, and listed as unchanged
//! ```

use super::{KeyPath, Node, Segment};
use crate::style::Style;
use core::any::Any;
use core::fmt;
use core::hash::{BuildHasherDefault, Hasher};
use std::collections::HashMap;

/// A composite, made by `widget!`, which a screen's view may keep with `memo`: built from its params alone, so equal
/// params build an equal tree.
#[diagnostic::on_unimplemented(
    message = "`memo` keeps a composite made by `widget!`, and `{Self}` is not one",
    note = "a primitive draws from more than its fields, so it is built at every view"
)]
pub trait Composite: crate::Element {}

/// How a kept composite is styled where it is placed: as its own markup styles it, or with the style its element names.
/// It is handed to one call and kept beside one composite, so its style is held in place: boxed, it would cost an
/// allocation for each kept composite at every view.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Look {
    Own,
    Given(Style),
}

/// What a composite marked `memo` was built from: the composite, its params and events, and how it was styled.
struct Memoed<T> {
    composite: T,
    look: Look,
}

/// What a kept composite was built from, held without its type.
pub struct Inputs(Box<dyn Any + Send>);

impl fmt::Debug for Inputs {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Inputs")
    }
}

/// Whether `kept` is `now`: what a screen's view compares each input of a kept composite with.
#[doc(hidden)]
#[must_use]
pub fn same<T: PartialEq + ?Sized>(kept: &T, now: &T) -> bool {
    kept == now
}

/// What each composite marked `memo` in the last tree a screen built was built from, by its key path.
#[derive(Debug, Default)]
pub struct Memos {
    by: HashMap<KeyPath, Inputs, BuildHasherDefault<Folding>>,
}

impl Memos {
    /// The node of the composite `build` makes at `path`, styled as `look` says: a marker that takes the last tree's
    /// node at `path` whole, when the composite kept there is of its type, was styled alike, and is `same` as it;
    /// otherwise built, carrying what it was built from. `build` is called twice when it builds: once for the tree, and
    /// once for what is kept, so a composite need not be `Clone`.
    pub(crate) fn node<T: Composite + Send + 'static>(
        &self,
        path: &[Segment],
        look: Look,
        build: impl Fn() -> T,
        same: impl FnOnce(&T) -> bool,
    ) -> Node {
        let segment = path.last().copied().unwrap_or(Segment::Place(0));
        let kept = self.by.get(path).and_then(|inputs| inputs.0.downcast_ref::<Memoed<T>>());
        let unchanged = kept.is_some_and(|kept| kept.look == look && same(&kept.composite));
        if unchanged {
            return Node::kept(segment);
        }
        let node = match &look {
            Look::Own => build().node(segment, Vec::new()),
            Look::Given(style) => build().styled(segment, style.clone(), Vec::new()),
        };

        node.memoed(Inputs(Box::new(Memoed { composite: build(), look })))
    }

    /// Keeps `inputs` as what the composite at `path` was built from.
    pub(crate) fn keep(&mut self, path: &[Segment], inputs: Inputs) {
        match self.by.get_mut(path) {
            Some(kept) => *kept = inputs,
            None => {
                self.by.insert(KeyPath::of(path), inputs);
            }
        }
    }

    /// Lets go of what the composite at `path` was built from.
    pub(crate) fn forget(&mut self, path: &[Segment]) {
        self.by.remove(path);
    }

    /// Lets go of everything kept, as the tree it was kept from is gone.
    pub(crate) fn clear(&mut self) {
        self.by.clear();
    }
}

/// A hash of a key path for the memos' map: a key path is a few numbers, so it folds each word in with one multiply,
/// as rustc's own maps do, where the standard hasher's resistance to chosen keys buys nothing.
#[derive(Debug, Default)]
pub struct Folding {
    hash: u64,
}

impl Folding {
    /// The odd constant each word is folded in with.
    const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

    /// `word` folded into the hash.
    const fn fold(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(Self::SEED);
    }
}

impl Hasher for Folding {
    fn finish(&self) -> u64 {
        self.hash
    }

    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.fold(u64::from_le_bytes(word));
        }
    }

    fn write_u8(&mut self, word: u8) {
        self.fold(u64::from(word));
    }

    fn write_u32(&mut self, word: u32) {
        self.fold(u64::from(word));
    }

    fn write_u64(&mut self, word: u64) {
        self.fold(word);
    }

    fn write_usize(&mut self, word: usize) {
        self.fold(u64::try_from(word).unwrap_or(u64::MAX));
    }

    fn write_isize(&mut self, word: isize) {
        self.fold(u64::from_ne_bytes(word.to_ne_bytes()));
    }
}
