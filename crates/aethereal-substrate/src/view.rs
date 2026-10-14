//! A view: state many actors read, folded from the effects of finished turns (ADR-0011).

use crate::Context;
use aethereal_actor::Effect;
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
///
/// Each effect is folded with the [`Context`] it was emitted in: the item its
/// turn was reacting to, which every effect of that turn shares. The context is
/// what is folded, not something read.
///
/// A view may read other views, named in [`Self::Reads`]. The engine folds
/// those first, so [`Self::takes`] sees them with the step's effects already
/// in. Views that read nothing of each other fold side by side, and a chain of
/// views reading views folds one link at a time, so the chain is kept short.
pub trait View {
    /// One part of the view's state.
    type Shard;
    /// What the view keeps of an effect.
    type Record;
    /// The views this one reads, as a tuple of references to them: `()` for a view that reads none.
    type Reads<'a>: Copy;

    /// The keys and records of an effect, one for each key it changes: none for an effect this view does not fold.
    ///
    /// `context` is the context the effect was emitted in, and `reads` are the views this one reads, each already
    /// folded for the step.
    fn takes<'a>(
        effect: &'a Effect,
        context: &'a Context,
        reads: Self::Reads<'a>,
    ) -> impl Iterator<Item = (u64, Self::Record)> + 'a;

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

    /// Folds in the effects of one turn, emitted in `context`, in the order given, reading `reads`.
    ///
    /// The caller folds every view in `reads` first.
    pub fn fold<'a>(&mut self, effects: &'a [Effect], context: &'a Context, reads: V::Reads<'a>) {
        for (key, record) in effects.iter().flat_map(|effect| V::takes(effect, context, reads)) {
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

    /// How many times each actor has closed.
    struct Closes;

    impl View for Closes {
        type Shard = BTreeMap<u64, u32>;
        type Record = ();
        type Reads<'a> = ();

        fn takes<'a>(effect: &'a Effect, _: &'a Context, (): ()) -> impl Iterator<Item = (u64, ())> + 'a {
            let is_close = *effect.what() == What::Close;

            is_close.then_some((effect.by().bits(), ())).into_iter()
        }

        fn fold(shard: &mut BTreeMap<u64, u32>, key: u64, (): ()) {
            *shard.entry(key).or_default() += 1;
        }
    }

    /// For each close, the count of closes as `Closes` holds it, under the closing actor and the one after it.
    struct Echoes;

    impl View for Echoes {
        type Shard = BTreeMap<u64, u32>;
        type Record = u32;
        type Reads<'a> = (&'a Folded<Closes>,);

        fn takes<'a>(
            effect: &'a Effect,
            _: &'a Context,
            (closes,): Self::Reads<'a>,
        ) -> impl Iterator<Item = (u64, u32)> + 'a {
            let by = effect.by().bits();
            let count = closes.shard(by).get(&by).copied();

            count.into_iter().flat_map(move |count| [(by, count), (by + 1, count)])
        }

        fn fold(shard: &mut BTreeMap<u64, u32>, key: u64, count: u32) {
            shard.insert(key, count);
        }
    }

    #[test]
    fn view_reads_another_already_folded_and_writes_several_keys() {
        // Catches a view that reads another before the step is folded into it, and a record under a second key
        // put in the part of the first.
        let Some(parts) = NonZeroUsize::new(2) else {
            panic!("two is not zero");
        };
        let effects = [Effect::new(ActorId::new(4), What::Close)];
        let context = Context { cause: None, root: MailId { sender: ActorId::new(4), count: 0 } };
        let mut closes = Folded::<Closes>::new(parts);
        let mut echoes = Folded::<Echoes>::new(parts);
        closes.fold(&effects, &context, ());
        echoes.fold(&effects, &context, (&closes,));

        assert_eq!(echoes.shard(4).get(&4), Some(&1));
        assert_eq!(echoes.shard(5).get(&5), Some(&1));
    }
}
