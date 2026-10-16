//! The view of who watches each actor whose ending is still to come (ADR-0009, ADR-0011).

use crate::{ByActor, Context, View};
use aethereal_actor::{Effect, Ended, How, SeatNumber, Seated, Seen, What};
use aethereal_data::ActorId;
use std::collections::BTreeMap;

/// The view of who watches each actor whose ending is still to come: what pairs a watch with an ending (ADR-0009).
///
/// It reads no other view. A watch and an ending are kept under the actor
/// watched, so the two meet in one part, and are paired once the step's
/// records are all in, by [`Shard::settle`]: an ending goes to every actor that
/// watches the one that ended, a watch asked for in the same step among them,
/// and a watch of an actor that ended before the step is shown that it did.
/// What is paired is handed on to the streams, as a [`Notice`] for the watcher.
///
/// It keeps nothing of an actor once it has ended (ADR-0008). A watch says how
/// the turn that asked for it saw the actor, so a watch of one that ended
/// before the step needs nothing kept to be answered.
pub struct Watches;

/// One part of the view.
#[derive(Default)]
pub struct Shard {
    /// Who watches each actor whose ending is still to come, by the actor watched: the watchers' seats, by their
    /// ids' bits.
    watched: ByActor<BTreeMap<u64, SeatNumber>>,
    /// The watches asked for in this step, by the actor watched: that actor's seat, who asked, in what context, and
    /// how the asking turn saw it.
    watches: Vec<(u64, SeatNumber, Seated, Context, Seen)>,
    /// The endings of this step: who ended, how, and in what context.
    endings: Vec<(Seated, How, Context)>,
}

/// What the view keeps of an effect, under the actor watched.
pub enum Record {
    /// The actor `by` names watches the key's actor, which sits in `seat`, as asked in this context, seeing it as
    /// `seen`.
    Watch { seat: SeatNumber, by: Seated, context: Context, seen: Seen },
    /// The key's actor, which sits in `seat`, has had its closing turn run: it ended this way, in this context.
    Ended { seat: SeatNumber, how: How, context: Context },
}

/// The ending of an actor, for an actor that watches it, handed on to the streams.
pub struct Notice {
    /// The watcher.
    pub to: Seated,
    /// Who ended, and how.
    pub ended: Ended,
    /// The context it ended in: the chain that ended it.
    pub context: Context,
}

impl Shard {
    /// Pairs the step's watches and endings, each under the actor watched, and adds to `forward` an ending for each
    /// watcher.
    pub fn settle(&mut self, forward: &mut Vec<Notice>) {
        // A watch is kept while its actor's ending is still to come: the actor was live to the turn that asked, it
        // is watched already, or it ends in this step. A watch of one that ended before the step is shown so at
        // once, with how it ended no longer kept.
        for (watched, seat, by, context, seen) in self.watches.drain(..) {
            let is_watched = self.watched.contains_key(&watched);
            let was_live = seen == Seen::Live;
            let is_kept = is_watched || was_live || ends_among(&self.endings, watched);
            if is_kept {
                self.watched.entry(watched).or_default().insert(by.id.bits(), by.seat);
            } else {
                let ended = Ended::new(Seated { id: ActorId::new(watched), seat }, How::Earlier);
                forward.push(Notice { to: by, ended, context });
            }
        }

        // An ending of this step goes to every watcher, those whose watch came in this step among them, and then
        // nothing of the actor is kept.
        for (ended, how, context) in self.endings.drain(..) {
            let Some(watching) = self.watched.remove(&ended.id.bits()) else {
                continue;
            };
            forward.extend(watching.into_iter().map(|(watcher, seat)| {
                let to = Seated { id: ActorId::new(watcher), seat };

                Notice { to, ended: Ended::new(ended, how.clone()), context }
            }));
        }
    }
}

/// Whether the actor `watched` stands for is among `endings`, the step's.
fn ends_among(endings: &[(Seated, How, Context)], watched: u64) -> bool {
    endings.iter().any(|(ended, _, _)| ended.id.bits() == watched)
}

impl View for Watches {
    type Shard = Shard;
    type Record = Record;

    fn takes<'a>(effect: &'a Effect, context: &'a Context) -> impl Iterator<Item = (u64, Record)> + 'a {
        let by = effect.seated();
        let record = match effect.what() {
            What::Watch { of, seen } => {
                let record = Record::Watch { seat: of.seat(), by, context: *context, seen: *seen };

                Some((of.id().bits(), record))
            }
            What::Ended(how) => {
                Some((by.id.bits(), Record::Ended { seat: by.seat, how: how.clone(), context: *context }))
            }
            _ => None,
        };

        record.into_iter()
    }

    /// Watches and endings wait for the step to settle.
    fn fold(shard: &mut Shard, key: u64, record: Record) {
        match record {
            Record::Watch { seat, by, context, seen } => shard.watches.push((key, seat, by, context, seen)),
            Record::Ended { seat, how, context } => {
                shard.endings.push((Seated { id: ActorId::new(key), seat }, how, context));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aethereal_actor::MailId;

    /// The actor `bits` names, in the seat numbered `seat`.
    const fn seated(bits: u64, seat: u64) -> Seated {
        Seated { id: ActorId::new(bits), seat: SeatNumber::new(seat) }
    }

    /// How each ending handed on goes: the watcher's seat and bits, and how.
    fn handed_on(forward: Vec<Notice>) -> Vec<(u64, u64, How)> {
        forward
            .into_iter()
            .map(|ending| (ending.to.seat.number(), ending.to.id.bits(), ending.ended.how().clone()))
            .collect()
    }

    #[test]
    fn watch_and_ending_of_one_step_pair_in_either_order() {
        // Catches a pairing made record by record, where an ending folded before a watch of the same step leaves
        // that watcher shown `Earlier`, and an ending kept from one step that shows a later watch how it went.
        let context = Context { root: MailId { sender: ActorId::new(7), count: 0 }, depth: 1 };
        let mut shard = Shard::default();
        let seven = SeatNumber::new(70);
        Watches::fold(&mut shard, 7, Record::Ended { seat: seven, how: How::Closed, context });
        let watch = Record::Watch { seat: seven, by: seated(9, 90), context, seen: Seen::Ended };
        Watches::fold(&mut shard, 7, watch);
        let mut forward = Vec::new();
        shard.settle(&mut forward);
        assert_eq!(handed_on(forward), [(90, 9, How::Closed)]);

        let watch = Record::Watch { seat: seven, by: seated(11, 110), context, seen: Seen::Ended };
        Watches::fold(&mut shard, 7, watch);
        let mut forward = Vec::new();
        shard.settle(&mut forward);
        assert_eq!(handed_on(forward), [(110, 11, How::Earlier)]);
    }

    #[test]
    fn nothing_is_kept_of_an_actor_once_its_watchers_are_shown_its_ending() {
        // Catches an ended actor's entry left behind, which grows the view by every actor that ever ended, and an
        // ending kept for an actor no one watched.
        let context = Context { root: MailId { sender: ActorId::new(7), count: 0 }, depth: 1 };
        let mut shard = Shard::default();
        let seven = SeatNumber::new(70);
        let watch = Record::Watch { seat: seven, by: seated(9, 90), context, seen: Seen::Live };
        Watches::fold(&mut shard, 7, watch);
        Watches::fold(&mut shard, 8, Record::Ended { seat: SeatNumber::new(80), how: How::Closed, context });
        shard.settle(&mut Vec::new());
        assert_eq!(shard.watched.len(), 1);

        Watches::fold(&mut shard, 7, Record::Ended { seat: seven, how: How::Closed, context });
        let mut forward = Vec::new();
        shard.settle(&mut forward);
        assert_eq!(handed_on(forward), [(90, 9, How::Closed)]);
        assert!(shard.watched.is_empty());
    }
}
