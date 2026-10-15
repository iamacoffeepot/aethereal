//! The view of who watches each actor, and which actors have ended (ADR-0009, ADR-0011).

use crate::{ByActor, Context, View};
use aethereal_actor::{Effect, Ended, How, SeatNumber, Seated, What};
use aethereal_data::ActorId;
use std::collections::BTreeMap;

/// The view of who watches each actor, and which actors have ended: what pairs a watch with an ending (ADR-0009).
///
/// It reads no other view. A watch and an ending are kept under the actor
/// watched, so the two meet in one part, and are paired once the step's
/// records are all in, by [`Shard::settle`]: an ending goes to every actor that
/// watches the one that ended, a watch asked for in the same step among them,
/// and a watch of an actor that ended before the step is shown that it did.
/// What is paired is handed on to the streams, as a [`Notice`] for the watcher.
pub struct Watches;

/// One part of the view.
#[derive(Default)]
pub struct Shard {
    /// Who watches each actor, and whether it has ended, by the actor watched.
    watched: ByActor<Watched>,
    /// The watches asked for in this step, by the actor watched: that actor's seat, who asked, and in what context.
    watches: Vec<(u64, SeatNumber, Seated, Context)>,
    /// The endings of this step: who ended, how, and in what context.
    endings: Vec<(Seated, How, Context)>,
}

/// One watched actor's entry.
#[derive(Default)]
struct Watched {
    /// The actors that watch it: their seats, by their ids' bits.
    by: BTreeMap<u64, SeatNumber>,
    /// Whether its life is over.
    life: Watching,
}

/// Whether a watched actor's life is over.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Watching {
    /// Its closing turn has not run.
    #[default]
    Running,
    /// Its closing turn has run.
    Over,
}

/// What the view keeps of an effect, under the actor watched.
pub enum Record {
    /// The actor `by` names watches the key's actor, which sits in `seat`, as asked in this context.
    Watch { seat: SeatNumber, by: Seated, context: Context },
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
        // A watch of an actor that ended before this step is shown so, with how it ended no longer kept.
        for (watched, seat, by, context) in self.watches.drain(..) {
            let entry = self.watched.entry(watched).or_default();
            let is_over = entry.life == Watching::Over;
            if is_over {
                let ended = Ended::new(Seated { id: ActorId::new(watched), seat }, How::Earlier);
                forward.push(Notice { to: by, ended, context });
            }
            entry.by.insert(by.id.bits(), by.seat);
        }

        // An ending of this step goes to every watcher, those whose watch came in this step among them.
        for (ended, how, context) in self.endings.drain(..) {
            let entry = self.watched.entry(ended.id.bits()).or_default();
            entry.life = Watching::Over;
            forward.extend(entry.by.iter().map(|(watcher, seat)| {
                let to = Seated { id: ActorId::new(*watcher), seat: *seat };

                Notice { to, ended: Ended::new(ended, how.clone()), context }
            }));
        }
    }
}

impl View for Watches {
    type Shard = Shard;
    type Record = Record;

    fn takes<'a>(effect: &'a Effect, context: &'a Context) -> impl Iterator<Item = (u64, Record)> + 'a {
        let by = effect.seated();
        let record = match effect.what() {
            What::Watch { of } => Some((of.id().bits(), Record::Watch { seat: of.seat(), by, context: *context })),
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
            Record::Watch { seat, by, context } => shard.watches.push((key, seat, by, context)),
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
        let context = Context { cause: None, root: MailId { sender: ActorId::new(7), count: 0 }, depth: 1 };
        let mut shard = Shard::default();
        let seven = SeatNumber::new(70);
        Watches::fold(&mut shard, 7, Record::Ended { seat: seven, how: How::Closed, context });
        Watches::fold(&mut shard, 7, Record::Watch { seat: seven, by: seated(9, 90), context });
        let mut forward = Vec::new();
        shard.settle(&mut forward);
        assert_eq!(handed_on(forward), [(90, 9, How::Closed)]);

        Watches::fold(&mut shard, 7, Record::Watch { seat: seven, by: seated(11, 110), context });
        let mut forward = Vec::new();
        shard.settle(&mut forward);
        assert_eq!(handed_on(forward), [(110, 11, How::Earlier)]);
    }
}
