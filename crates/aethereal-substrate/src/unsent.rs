//! The view of the bytes each actor that sends holds unsent, as it last told the engine, ranked so the longest
//! backlogs are read without walking the actors (ADR-0053).

use crate::{ByActor, Context, View};
use aethereal_actor::{Against, Arena, Effect, LARGEST, What};
use aethereal_data::ActorId;
use std::collections::BTreeSet;

/// The view of the bytes each actor holds unsent, keyed by the actor's seat, so each part ranks its own actors
/// (ADR-0011, ADR-0053).
///
/// An actor is in it only while what it last said it holds is more than none. Its entry is its own: what it says, and
/// its ending, which takes the entry out.
pub struct Unsent;

/// One part of the view: each actor holding bytes unsent, and the same actors ranked by how many.
#[derive(Default)]
pub struct Shard {
    /// What each actor holds unsent, by its id's bits.
    holding: ByActor<Against>,
    /// Each actor in `holding`, as its bytes and its id's bits, so the last are the longest.
    ranked: BTreeSet<(u64, u64)>,
}

/// What the view keeps of an effect.
pub enum Record {
    /// The actor holds this many bytes unsent, of the most it may.
    Holds { id: ActorId, unsent: Against },
    /// The actor's closing turn has run, and it holds nothing.
    Over(ActorId),
}

impl Shard {
    /// The actors of the part holding the most bytes unsent, the most first, at most [`LARGEST`]: two that hold the
    /// same, the one of the larger id first.
    pub fn longest(&self) -> impl Iterator<Item = (ActorId, Against)> + '_ {
        let ranked = self.ranked.iter().rev().take(LARGEST);

        ranked.filter_map(|(_, bits)| Some((ActorId::new(*bits), *self.holding.get(bits)?)))
    }

    /// Takes the actor whose id's bits are `bits` off the list.
    fn forget(&mut self, bits: u64) {
        if let Some(was) = self.holding.remove(&bits) {
            self.ranked.remove(&(was.bytes, bits));
        }
    }
}

impl View for Unsent {
    type Shard = Shard;
    type Record = Record;

    fn takes<'a>(effect: &'a Effect, _: &'a Context) -> impl Iterator<Item = (u64, Record)> + 'a {
        let key = effect.seated().seat.number();
        let record = match effect.what() {
            What::Unsent { bytes, most } => {
                Some((key, Record::Holds { id: effect.by(), unsent: Against { bytes: *bytes, most: *most } }))
            }
            What::Ended(_) => Some((key, Record::Over(effect.by()))),
            _ => None,
        };

        record.into_iter()
    }

    fn fold(shard: &mut Shard, _: u64, record: Record, _: &[Arena]) {
        match record {
            Record::Holds { id, unsent } => {
                shard.forget(id.bits());
                if unsent.bytes > 0 {
                    shard.holding.insert(id.bits(), unsent);
                    shard.ranked.insert((unsent.bytes, id.bits()));
                }
            }
            Record::Over(id) => shard.forget(id.bits()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Folded;
    use aethereal_actor::{How, MailId, SeatNumber, Seated};
    use std::num::NonZeroUsize;

    /// The actor numbered `n`, seated in seat `n`.
    fn actor(n: u64) -> Seated {
        Seated { id: ActorId::new(n), seat: SeatNumber::new(n) }
    }

    /// The effect by which the actor numbered `n` says it holds `bytes` unsent, of 100.
    fn holds(n: u64, bytes: u64) -> Effect {
        Effect::new(actor(n), What::Unsent { bytes, most: 100 })
    }

    /// The effect that says the actor numbered `n` has run its closing turn.
    fn over(n: u64) -> Effect {
        Effect::new(actor(n), What::Ended(How::Closed))
    }

    /// The longest backlogs across every part of `view`, the most first, as the engine merges them: each actor's
    /// number and bytes.
    fn longest(view: &Folded<Unsent>) -> Vec<(u64, u64)> {
        let mut merged: Vec<(u64, u64)> =
            view.shards().flat_map(Shard::longest).map(|(id, unsent)| (id.bits(), unsent.bytes)).collect();
        merged.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));
        merged.truncate(LARGEST);

        merged
    }

    #[test]
    fn the_longest_backlogs_stay_right_as_they_grow_shrink_and_end() {
        // Catches a ranking that keeps an actor's old figure beside its new one, keeps an actor that said it holds
        // none or ended, loses the ninth when one of the eight shrinks below it, or ranks a part's actors apart from
        // another's.
        let parts = NonZeroUsize::new(4).unwrap_or(NonZeroUsize::MIN);
        let mut view = Folded::<Unsent>::new(parts);
        let context = Context { root: MailId { sender: ActorId::new(0), count: 0 }, depth: 1 };
        let fold = |view: &mut Folded<Unsent>, effects: &[Effect]| view.fold(effects, &context, &[]);

        // Ten actors, across the four parts, each holding ten times its number.
        let grown: Vec<Effect> = (1..=10).map(|n| holds(n, n * 10)).collect();
        fold(&mut view, &grown);
        let first = longest(&view);

        // The longest shrinks below the ninth, one ends, and one says it holds none.
        fold(&mut view, &[holds(10, 15), over(9), holds(8, 0)]);
        let after = longest(&view);

        let expected_first = vec![(10, 100), (9, 90), (8, 80), (7, 70), (6, 60), (5, 50), (4, 40), (3, 30)];
        let expected_after = vec![(7, 70), (6, 60), (5, 50), (4, 40), (3, 30), (2, 20), (10, 15), (1, 10)];
        assert_eq!((first, after), (expected_first, expected_after));
    }
}
