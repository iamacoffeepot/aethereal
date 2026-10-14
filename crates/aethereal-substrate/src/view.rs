//! A view: state many actors read, folded from the effects of finished turns (ADR-0011).

use crate::Context;
use aethereal_actor::Effect;
use std::num::NonZeroUsize;

/// A view: what it keeps of an effect, how one such record changes its state, and what a part hands on once a
/// step is folded (ADR-0011).
///
/// Its author writes these and nothing about threads. The engine keeps the
/// view in a [`Folded`], which does the rest the same way for every view.
///
/// A key has one owner, whose records reach [`Self::fold`] in the order it
/// asked for them. Or many actors write a key, and then the fold gives the
/// same state in whatever order their records arrive.
///
/// Each effect is folded with the [`Context`] it was emitted in: the item its
/// turn was reacting to, which every effect of that turn shares. A view reads
/// nothing else, and no other view: each view folds by itself, in whatever
/// order and wherever the engine folds it.
pub trait View {
    /// One part of the view's state.
    type Shard;
    /// What the view keeps of an effect.
    type Record;

    /// The keys and records of an effect, one for each key it changes: none for an effect this view does not fold.
    ///
    /// `context` is the context the effect was emitted in.
    fn takes<'a>(effect: &'a Effect, context: &'a Context) -> impl Iterator<Item = (u64, Self::Record)> + 'a;

    /// Folds one record into the part its key falls in.
    fn fold(shard: &mut Self::Shard, key: u64, record: Self::Record);

    /// Settles a part once every record of a step has been folded into it, in whatever order they came, and adds
    /// to `forward` the records it hands on to other keys, which may fall in other parts. Most views settle nothing
    /// and hand on nothing.
    fn settle(shard: &mut Self::Shard, forward: &mut Vec<(u64, Self::Record)>) {
        let _ = (shard, forward);
    }
}

/// A view that can take an effect by value, so what it keeps of the effect is moved in and not copied.
///
/// Only the last view to fold a turn's effects can take them, since nothing
/// reads an effect after it is taken. [`View::takes`] gives the same records
/// from a borrowed effect.
pub trait Consumes: View {
    /// The keys and records of `effect`, taken by value, as [`View::takes`] gives them.
    fn takes_owned(effect: Effect, context: &Context) -> impl Iterator<Item = (u64, Self::Record)> + '_;
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

    /// Folds in the effects of one turn, emitted in `context`, in the order given.
    pub fn fold(&mut self, effects: &[Effect], context: &Context) {
        for (key, record) in effects.iter().flat_map(|effect| V::takes(effect, context)) {
            let part = self.part(key);
            V::fold(&mut self.shards[part], key, record);
        }
    }

    /// Folds in the effects of one turn, emitted in `context`, taking each by value.
    ///
    /// The caller folds every other view that reads these effects first.
    pub fn fold_owned(&mut self, effects: impl IntoIterator<Item = Effect>, context: &Context)
    where
        V: Consumes,
    {
        for (key, record) in effects.into_iter().flat_map(|effect| V::takes_owned(effect, context)) {
            let part = self.part(key);
            V::fold(&mut self.shards[part], key, record);
        }
    }

    /// Settles every part once a step's effects are all folded, and folds what each hands on into the part its key
    /// falls in. What is handed on is folded and not settled again.
    pub fn settle(&mut self) {
        let mut forward = Vec::new();
        for shard in &mut self.shards {
            V::settle(shard, &mut forward);
        }

        for (key, record) in forward {
            let part = self.part(key);
            V::fold(&mut self.shards[part], key, record);
        }
    }

    /// The part `key` falls in, as of the last fold.
    #[must_use]
    pub fn shard(&self, key: u64) -> &V::Shard {
        &self.shards[self.part(key)]
    }

    /// The part `key` falls in, to change outside a fold: for the engine, which takes from a view what it hands
    /// out, such as the items an actor is to react to, between steps.
    pub fn shard_mut(&mut self, key: u64) -> &mut V::Shard {
        let part = self.part(key);

        &mut self.shards[part]
    }

    /// Every part, as of the last fold.
    pub fn shards(&self) -> impl Iterator<Item = &V::Shard> {
        self.shards.iter()
    }

    /// Every part, to change outside a fold, as [`Self::shard_mut`] gives one.
    pub fn shards_mut(&mut self) -> impl Iterator<Item = &mut V::Shard> {
        self.shards.iter_mut()
    }

    /// Which part `key` falls in.
    fn part(&self, key: u64) -> usize {
        #[allow(clippy::cast_possible_truncation, reason = "the remainder is less than the count of parts, a usize")]
        let part = (key % self.shards.len() as u64) as usize;

        part
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aethereal_actor::{MailId, What};
    use aethereal_data::ActorId;
    use std::collections::BTreeMap;

    /// Each close, kept under the closing actor until the step settles, then counted under the actor after it.
    struct Relays;

    /// What [`Relays`] keeps: a close not yet handed on, or one counted.
    enum Relay {
        Held,
        Counted,
    }

    impl View for Relays {
        type Shard = (Vec<u64>, BTreeMap<u64, u32>);
        type Record = Relay;

        fn takes<'a>(effect: &'a Effect, _: &'a Context) -> impl Iterator<Item = (u64, Relay)> + 'a {
            let is_close = *effect.what() == What::Close;

            is_close.then_some((effect.by().bits(), Relay::Held)).into_iter()
        }

        fn fold((held, counted): &mut Self::Shard, key: u64, record: Relay) {
            match record {
                Relay::Held => held.push(key),
                Relay::Counted => *counted.entry(key).or_default() += 1,
            }
        }

        fn settle((held, _): &mut Self::Shard, forward: &mut Vec<(u64, Relay)>) {
            forward.extend(held.drain(..).map(|key| (key + 1, Relay::Counted)));
        }
    }

    #[test]
    fn what_a_part_hands_on_lands_in_the_part_of_its_key() {
        // Catches a record handed on and kept in the part that settled it, and one handed on and never folded.
        let Some(parts) = NonZeroUsize::new(2) else {
            panic!("two is not zero");
        };
        let effects = [Effect::new(ActorId::new(4), What::Close)];
        let context = Context { cause: None, root: MailId { sender: ActorId::new(4), count: 0 }, depth: 1 };
        let mut relays = Folded::<Relays>::new(parts);
        relays.fold(&effects, &context);
        assert_eq!(relays.shard(5).1.get(&5), None);

        relays.settle();
        assert_eq!(relays.shard(5).1.get(&5), Some(&1));
        assert!(relays.shard(4).0.is_empty());
    }
}
