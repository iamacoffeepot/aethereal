//! The blobs the engine holds, and the pins that keep each one (ADR-0014).

use aethereal_actor::pins::Change;
use aethereal_actor::{BlobHash, BlobId, Puts};
use std::collections::HashMap;

/// The blobs the engine holds, each with its pins, and the budget their bytes come out of (ADR-0014).
///
/// Turns read it and nothing writes it while they run. When a step's turns are
/// done, it takes in the blobs each writer put, sums the changes to their pins,
/// and frees each blob left with none.
pub struct Blobs {
    /// Which engine of the process holds them, in the id of each.
    engine: u32,
    held: HashMap<u64, Held>,
    /// The bytes the host lets all blobs hold.
    budget: usize,
    /// The bytes the blobs held hold.
    in_use: usize,
    /// The blobs taken in, or whose pins changed, since the last sweep: the only ones a sweep can free.
    touched: Vec<u64>,
}

/// One blob, how many pins it has, and whether its hash is known.
struct Held {
    bytes: Box<[u8]>,
    pins: i64,
    content: Content,
}

/// What the store knows of a blob's content beyond its bytes: its hash, once something has asked for it (ADR-0014).
#[derive(Clone, Copy)]
enum Content {
    Unhashed,
    Hashed(BlobHash),
}

impl Blobs {
    /// No blobs, with `budget` bytes for them.
    #[must_use]
    pub fn new(engine: u32, budget: usize) -> Self {
        Self { engine, held: HashMap::new(), budget, in_use: 0, touched: Vec::new() }
    }

    /// The bytes of the blob `id` names, when it is held.
    #[must_use]
    pub fn read(&self, id: BlobId) -> Option<&[u8]> {
        self.held.get(&id.bits()).map(|held| &held.bytes[..])
    }

    /// The hash kept for the blob `id` names, when it is held and something has asked for its hash.
    #[must_use]
    pub fn hash(&self, id: BlobId) -> Option<BlobHash> {
        match self.held.get(&id.bits())?.content {
            Content::Hashed(hash) => Some(hash),
            Content::Unhashed => None,
        }
    }

    /// The hash of the blob `id` names, when it is held: the one kept, or one computed now and kept.
    pub fn hash_now(&mut self, id: BlobId) -> Option<BlobHash> {
        let held = self.held.get_mut(&id.bits())?;
        let hash = match held.content {
            Content::Hashed(hash) => hash,
            Content::Unhashed => BlobHash::of(&held.bytes),
        };
        held.content = Content::Hashed(hash);

        Some(hash)
    }

    /// Every hash kept, in no order.
    #[cfg(test)]
    pub(crate) fn kept_hashes(&self) -> Vec<BlobHash> {
        let kept = self.held.values().map(|held| held.content);

        kept.filter_map(|content| match content {
            Content::Hashed(hash) => Some(hash),
            Content::Unhashed => None,
        })
        .collect()
    }

    /// The bytes the budget has left.
    #[must_use]
    pub const fn room(&self) -> usize {
        self.budget.saturating_sub(self.in_use)
    }

    /// How many blobs are held, and their bytes.
    #[must_use]
    pub fn held(&self) -> (usize, usize) {
        (self.held.len(), self.in_use)
    }

    /// Takes in what one writer left: the blobs it put, with no pins yet, as the pins each was put with come with the
    /// changes; then the hashes its turns computed, of those blobs or of ones already held.
    ///
    /// # Panics
    ///
    /// Panics when a blob comes in under an id one held has, which the writers' counts rule out.
    pub fn take_in(&mut self, puts: &mut Puts) {
        for (id, bytes) in puts.take() {
            self.in_use += bytes.len();
            let was_held = self.held.insert(id.bits(), Held { bytes, pins: 0, content: Content::Unhashed }).is_some();
            assert!(!was_held, "blob store: a blob was put under the id {id:?}, which is already held");
            self.touched.push(id.bits());
        }
        // A blob freed since its hash was computed has nothing to keep it with.
        for (id, hash) in puts.take_hashed() {
            if let Some(held) = self.held.get_mut(&id.bits()) {
                held.content = Content::Hashed(hash);
            }
        }
    }

    /// Sums `changes` into the blobs' pins, and leaves the list empty, with its room.
    pub fn change(&mut self, changes: &mut Vec<Change>) {
        for (id, by) in changes.drain(..) {
            if let Some(held) = self.held.get_mut(&id.bits()) {
                held.pins += by;
                self.touched.push(id.bits());
            }
        }
    }

    /// Frees every blob taken in or changed since the last sweep that has no pin left, once every change to the
    /// pins of the step has been summed, and tells `freed` of each.
    pub fn sweep(&mut self, mut freed: impl FnMut(BlobId)) {
        for bits in self.touched.drain(..) {
            let is_unpinned = self.held.get(&bits).is_some_and(|held| held.pins <= 0);
            if is_unpinned && let Some(held) = self.held.remove(&bits) {
                debug_assert_eq!(held.pins, 0, "blob store: a blob lost more pins than it had");
                self.in_use -= held.bytes.len();
                freed(BlobId::new(self.engine, bits));
            }
        }
    }
}
