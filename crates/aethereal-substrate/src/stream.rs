//! The view of what each actor has to react to (ADR-0009, ADR-0011).

use crate::{ByActor, Consumes, Context, Envelope, View};
use aethereal_actor::{Effect, Ended, How, MailId, SeatNumber, Seated, What};
use aethereal_data::ActorId;
use std::collections::BTreeMap;
use std::mem;
use std::ops::Range;

/// The view of what each actor has to react to: its stream of items, and who watches each actor (ADR-0009).
///
/// It reads no other view. Mail is kept under the seat of the actor its turn
/// named, whether or not that actor is still live: an actor that is not live
/// is never handed its items, and they are dropped there. The seat comes with
/// the actor's id wherever an actor is named, so the items of a step are handed
/// out in the order of the seats. A watch and an ending are kept under the
/// actor watched, so the two meet in one part, and are paired once the step's
/// records are all in: an ending goes to every actor that watches the one that
/// ended, a watch asked for in the same step among them, and a watch of an
/// actor that ended before the step is shown that it did.
///
/// Many actors write one key, so an actor's items are kept in an order that
/// does not depend on the order they arrive in: by their source, then in the
/// order that source caused them. An actor's ending is the last thing it
/// causes, so it comes after that actor's mail.
///
/// A part keeps the items of all its actors in one list, as they arrive. At
/// the start of a step [`Shard::hand`] puts the list in order, once, and the
/// step's turns read each actor's run of it where it lies. An entry in the list
/// owns nothing: an ending, which holds how its actor ended, is kept in a
/// list of its own beside it, so the list is emptied without a look at what it
/// held.
pub struct Streams;

/// One part of the view.
#[derive(Default)]
pub struct Shard {
    /// The items that arrived since the step began, for whichever actor, as they arrived.
    arrived: Vec<Entry>,
    /// The endings among the items that arrived since the step began, which their entries number.
    arrived_endings: Vec<(Ended, Context)>,
    /// The items handed out at the start of the step, in order: by seat and actor, then as each actor is handed them.
    handed: Vec<Entry>,
    /// The endings among the items handed out at the start of the step, which their entries number.
    handed_endings: Vec<(Ended, Context)>,
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

/// What the view keeps of an effect.
pub enum Record {
    /// An item for the actor whose bits are `to`, which sits in the seat numbered by the key, and where it stands in
    /// that actor's stream.
    Item { to: u64, order: Order, pending: Pending },
    /// The actor `by` names watches the key's actor, which sits in `seat`, as asked in this context.
    Watch { seat: SeatNumber, by: Seated, context: Context },
    /// The key's actor, which sits in `seat`, has had its closing turn run: it ended this way, in this context.
    Ended { seat: SeatNumber, how: How, context: Context },
}

/// One item in an actor's stream.
pub enum Pending {
    /// A piece of mail.
    Mail(Envelope),
    /// An actor it watches ended. What it does about it belongs to the context, the chain that ended that actor.
    /// Endings are rare, so one is kept in bytes of its own and does not widen every item.
    Ended(Box<(Ended, Context)>),
}

/// Where an item stands in its stream: its source's bits, then the order that source caused it in.
pub type Order = (u64, u64);

/// One item in a part, and the seat and actor it is for.
#[derive(Clone, Copy)]
pub struct Entry {
    /// The number of the seat of the actor it is for.
    seat: u64,
    /// The bits of the actor it is for.
    to: u64,
    /// Where it stands in that actor's stream. For mail, its id: its sender's bits and that sender's count.
    order: Order,
    item: Stored,
}

/// An item as a part keeps it.
#[derive(Clone, Copy)]
enum Stored {
    /// A piece of mail.
    Mail(Envelope),
    /// An ending: the place of it, and of its context, in the part's list of endings.
    Ended(usize),
}

/// An item handed out, as a turn reads it.
#[derive(Clone, Copy)]
pub enum Handed<'a> {
    /// A piece of mail, and its id.
    Mail { id: MailId, envelope: &'a Envelope },
    /// An actor it watches ended. What it does about it belongs to `context`, the chain that ended that actor.
    Ended { ended: &'a Ended, context: &'a Context },
}

impl Shard {
    /// Hands out the items that arrived since the last step, put in order, and answers with each actor's run of
    /// them: the number of the seat the actor sat in when they were sent, the actor's bits, and where its items lie
    /// in [`Self::handed`].
    ///
    /// The order is by seat and actor, then by source, then the order that source caused each item, so it does not
    /// depend on the order the items arrived in. A seat is taken again once its actor has gone, so a run names its
    /// actor. An ending shown twice, from two watches of an actor already over, is handed once. What was handed out
    /// at the last step is dropped here.
    pub fn hand(&mut self) -> impl Iterator<Item = (u64, u64, Range<usize>)> + '_ {
        self.handed.clear();
        self.handed_endings.clear();
        mem::swap(&mut self.arrived, &mut self.handed);
        mem::swap(&mut self.arrived_endings, &mut self.handed_endings);
        self.handed.sort_unstable_by_key(|entry| (entry.seat, entry.to, entry.order));
        self.handed.dedup_by_key(|entry| (entry.seat, entry.to, entry.order));

        let mut begins = 0;
        self.handed.chunk_by(|one, next| (one.seat, one.to) == (next.seat, next.to)).map(move |run| {
            let range = begins..begins + run.len();
            begins = range.end;

            (run[0].seat, run[0].to, range)
        })
    }

    /// The items handed out at the start of the step that lie in `range`.
    #[must_use]
    pub fn handed(&self, range: Range<usize>) -> impl ExactSizeIterator<Item = Handed<'_>> {
        self.handed[range].iter().map(|entry| entry.handed(&self.handed_endings))
    }

    /// The items that arrived since the step began, in no order: the actor each is for, and the item.
    pub fn arrived(&self) -> impl Iterator<Item = (Seated, Handed<'_>)> {
        self.arrived.iter().map(|entry| {
            let to = Seated { id: ActorId::new(entry.to), seat: SeatNumber::new(entry.seat) };

            (to, entry.handed(&self.arrived_endings))
        })
    }

    /// Whether any item has arrived since the step began.
    #[must_use]
    pub fn has_items(&self) -> bool {
        !self.arrived.is_empty()
    }
}

impl Entry {
    /// The item as a turn reads it, its ending found in `endings` when it is one.
    fn handed<'a>(&'a self, endings: &'a [(Ended, Context)]) -> Handed<'a> {
        match &self.item {
            Stored::Mail(envelope) => {
                let (sender, count) = self.order;

                Handed::Mail { id: MailId { sender: ActorId::new(sender), count }, envelope }
            }
            Stored::Ended(place) => {
                let (ended, context) = &endings[*place];

                Handed::Ended { ended, context }
            }
        }
    }
}

impl Handed<'_> {
    /// The actor the item comes from, and its seat.
    #[must_use]
    pub const fn source(self) -> Seated {
        match self {
            Self::Mail { id, envelope } => Seated { id: id.sender, seat: envelope.sender_seat },
            Self::Ended { ended, .. } => ended.actor().seated(),
        }
    }
}

impl View for Streams {
    type Shard = Shard;
    type Record = Record;

    fn takes<'a>(effect: &'a Effect, context: &'a Context) -> impl Iterator<Item = (u64, Record)> + 'a {
        Self::takes_owned(effect.clone(), context)
    }

    /// Items are kept as they arrive and put in order when they are handed out, by [`Shard::hand`]. Watches and
    /// endings wait for the step to settle.
    fn fold(shard: &mut Shard, key: u64, record: Record) {
        match record {
            Record::Item { to, order, pending } => {
                let item = match pending {
                    Pending::Mail(envelope) => Stored::Mail(envelope),
                    Pending::Ended(ending) => {
                        shard.arrived_endings.push(*ending);

                        Stored::Ended(shard.arrived_endings.len() - 1)
                    }
                };
                shard.arrived.push(Entry { seat: key, to, order, item });
            }
            Record::Watch { seat, by, context } => shard.watches.push((key, seat, by, context)),
            Record::Ended { seat, how, context } => {
                shard.endings.push((Seated { id: ActorId::new(key), seat }, how, context));
            }
        }
    }

    /// Pairs the step's watches and endings, each under the actor watched, and hands on an ending to each watcher.
    fn settle(shard: &mut Shard, forward: &mut Vec<(u64, Record)>) {
        // A watch of an actor that ended before this step is shown so, with how it ended no longer kept.
        for (watched, seat, by, context) in shard.watches.drain(..) {
            let entry = shard.watched.entry(watched).or_default();
            let is_over = entry.life == Watching::Over;
            if is_over {
                forward.push(handed(by, Seated { id: ActorId::new(watched), seat }, How::Earlier, context));
            }
            entry.by.insert(by.id.bits(), by.seat);
        }

        // An ending of this step goes to every watcher, those whose watch came in this step among them.
        for (ended, how, context) in shard.endings.drain(..) {
            let entry = shard.watched.entry(ended.id.bits()).or_default();
            entry.life = Watching::Over;
            forward.extend(entry.by.iter().map(|(watcher, seat)| {
                let watcher = Seated { id: ActorId::new(*watcher), seat: *seat };

                handed(watcher, ended, how.clone(), context)
            }));
        }
    }
}

impl Consumes for Streams {
    fn takes_owned(effect: Effect, context: &Context) -> impl Iterator<Item = (u64, Record)> + '_ {
        // At most one record, and a mail's bytes are moved into its envelope.
        let by = effect.seated();
        let record = match effect.into_what() {
            What::Send { to, count, kind, bytes } => {
                let (cause, root, depth) = (context.cause, context.root, context.depth);
                let envelope = Envelope { sender_seat: by.seat, cause, root, depth, kind, bytes };
                let order = (by.id.bits(), count);

                Some((to.seat.number(), Record::Item { to: to.id.bits(), order, pending: Pending::Mail(envelope) }))
            }
            What::Watch { of } => Some((of.id().bits(), Record::Watch { seat: of.seat(), by, context: *context })),
            What::Ended(how) => Some((by.id.bits(), Record::Ended { seat: by.seat, how, context: *context })),
            _ => None,
        };

        record.into_iter()
    }
}

/// The ending of the actor `ended` names, `how` it ended, as an item for the watcher `to` names, kept under the
/// watcher's seat: last of what that actor caused.
fn handed(to: Seated, ended: Seated, how: How, context: Context) -> (u64, Record) {
    let order = (ended.id.bits(), u64::MAX);
    let pending = Pending::Ended(Box::new((Ended::new(ended, how), context)));

    (to.seat.number(), Record::Item { to: to.id.bits(), order, pending })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The actor `bits` names, in the seat numbered `seat`.
    const fn seated(bits: u64, seat: u64) -> Seated {
        Seated { id: ActorId::new(bits), seat: SeatNumber::new(seat) }
    }

    /// How each ending handed on goes: the seat it is kept under, the watcher's bits, and how.
    fn handed_on(forward: Vec<(u64, Record)>) -> Vec<(u64, u64, How)> {
        forward
            .into_iter()
            .filter_map(|(seat, record)| match record {
                Record::Item { to, pending: Pending::Ended(ending), .. } => Some((seat, to, ending.0.how().clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn watch_and_ending_of_one_step_pair_in_either_order() {
        // Catches a pairing made record by record, where an ending folded before a watch of the same step leaves
        // that watcher shown `Earlier`, an ending kept from one step that shows a later watch how it went, and an
        // ending kept under any seat but its watcher's.
        let context = Context { cause: None, root: MailId { sender: ActorId::new(7), count: 0 }, depth: 1 };
        let mut shard = Shard::default();
        let seven = SeatNumber::new(70);
        Streams::fold(&mut shard, 7, Record::Ended { seat: seven, how: How::Closed, context });
        Streams::fold(&mut shard, 7, Record::Watch { seat: seven, by: seated(9, 90), context });
        let mut forward = Vec::new();
        Streams::settle(&mut shard, &mut forward);
        assert_eq!(handed_on(forward), [(90, 9, How::Closed)]);

        Streams::fold(&mut shard, 7, Record::Watch { seat: seven, by: seated(11, 110), context });
        let mut forward = Vec::new();
        Streams::settle(&mut shard, &mut forward);
        assert_eq!(handed_on(forward), [(110, 11, How::Earlier)]);
    }
}
