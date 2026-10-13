//! A view: state many actors read, folded from the effects of finished turns (ADR-0011).

use crate::Effect;
use std::num::NonZeroUsize;

/// A view: what it keeps of an effect, and how one such record changes its state (ADR-0011).
///
/// Its author writes these two things and nothing about threads. The engine
/// keeps the view in a [`Folded`], which does the rest the same way for every
/// view.
///
/// A key has one owner, whose records reach [`Self::fold`] in the order it
/// asked for them. Or many actors write a key, and then the fold gives the
/// same state in whatever order their records arrive.
pub trait View {
    /// One part of the view's state.
    type Shard;
    /// What the view keeps of an effect.
    type Record;

    /// The key and record of an effect this view folds, or nothing for one it does not.
    fn takes(effect: &Effect) -> Option<(u64, Self::Record)>;

    /// Folds one record into the part its key falls in.
    fn fold(shard: &mut Self::Shard, key: u64, record: Self::Record);
}

/// A view as the engine keeps it: its state in parts, each holding the keys that fall in it (ADR-0011).
///
/// While a step's turns run they read it, and nothing writes it. When they
/// have finished, the step's effects are folded in. Folding takes the view
/// exclusively, so the two never happen together and nothing here is locked.
pub struct Folded<V: View> {
    /// The parts. A key always falls in the same one.
    shards: Vec<V::Shard>,
}

impl<V: View> Folded<V> {
    /// The view before any effect, its keys spread over `parts` parts.
    #[must_use]
    pub fn new(parts: NonZeroUsize) -> Self
    where
        V::Shard: Default,
    {
        Self { shards: (0..parts.get()).map(|_| V::Shard::default()).collect() }
    }

    /// Folds in the effects of one step's turns, in the order given.
    pub fn fold(&mut self, effects: &[Effect]) {
        for (key, record) in effects.iter().filter_map(V::takes) {
            let part = self.part(key);
            V::fold(&mut self.shards[part], key, record);
        }
    }

    /// The part `key` falls in, as of the last fold.
    #[must_use]
    pub fn shard(&self, key: u64) -> &V::Shard {
        &self.shards[self.part(key)]
    }

    /// Which part `key` falls in.
    fn part(&self, key: u64) -> usize {
        #[allow(clippy::cast_possible_truncation, reason = "the remainder is less than the count of parts, a usize")]
        let part = (key % self.shards.len() as u64) as usize;

        part
    }
}
