//! The view of what each actor has to react to (ADR-0009, ADR-0011).

use crate::{Consumes, Context, Envelope, View};
use aethereal_actor::{Effect, Ended, How, What};
use aethereal_data::ActorId;
use std::collections::{BTreeMap, BTreeSet};
use std::mem;
use std::ops::Range;

/// The view of what each actor has to react to: its stream of items, and who watches each actor (ADR-0009).
///
/// It reads no other view. Mail is kept under the actor its turn named,
/// whether or not that actor is still live: an actor that is not live is never
/// handed its items, and they are dropped there. A watch and an ending are
/// kept under the actor watched, so the two meet in one part, and are paired
/// once the step's records are all in: an ending goes to every actor that
/// watches the one that ended, a watch asked for in the same step among them,
/// and a watch of an actor that ended before the step is shown that it did.
///
/// Many actors write one key, so an actor's items are kept in an order that
/// does not depend on the order they arrive in: by their source, then in the
/// order that source caused them. An actor's ending is the last thing it
/// causes, so it comes after that actor's mail.
///
/// A part keeps the items of all its actors in one list, as they arrive. At
/// the start of a step [`Shard::hand`] puts the list in order, once, and the
/// step's turns read each actor's run of it where it lies.
pub struct Streams;

/// One part of the view.
#[derive(Default)]
pub struct Shard {
    /// The items that arrived since the step began, for whichever actor, as they arrived.
    arrived: Vec<Entry>,
    /// The items handed out at the start of the step, in order: by actor, then as each actor is handed them.
    handed: Vec<Entry>,
    /// Who watches each actor, and whether it has ended, by the actor watched.
    watched: BTreeMap<u64, Watched>,
    /// The watches asked for in this step, by the actor watched: who asked, and in what context.
    watches: Vec<(u64, u64, Context)>,
    /// The endings of this step: who ended, how, and in what context.
    endings: Vec<(u64, How, Context)>,
}

/// One watched actor's entry.
#[derive(Default)]
struct Watched {
    /// The actors that watch it.
    by: BTreeSet<u64>,
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
    /// An item for the key's actor, and where it stands in that actor's stream.
    Item(Order, Pending),
    /// The actor whose bits these are watches the key's actor, as asked in this context.
    Watch { by: u64, context: Context },
    /// The key's actor's closing turn has run: it ended this way, in this context.
    Ended { how: How, context: Context },
}

/// One item in an actor's stream.
pub enum Pending {
    /// A piece of mail.
    Mail(Envelope),
    /// An actor it watches ended. What it does about it belongs to `context`, the chain that ended that actor.
    Ended { ended: Ended, context: Context },
}

/// Where an item stands in its stream: its source's bits, then the order that source caused it in.
pub type Order = (u64, u64);

/// One item in a part, and the actor it is for.
pub struct Entry {
    /// The bits of the actor it is for.
    to: u64,
    order: Order,
    pending: Pending,
}

impl Shard {
    /// Hands out the items that arrived since the last step, put in order, and answers with each actor's run of
    /// them: the actor's bits and where its items lie in [`Self::handed`].
    ///
    /// The order is by actor, then by source, then the order that source caused each item, so it does not depend
    /// on the order the items arrived in. An ending shown twice, from two watches of an actor already over, is
    /// handed once. What was handed out at the last step is dropped here.
    pub fn hand(&mut self) -> impl Iterator<Item = (u64, Range<usize>)> + '_ {
        self.handed.clear();
        mem::swap(&mut self.arrived, &mut self.handed);
        self.handed.sort_unstable_by_key(|entry| (entry.to, entry.order));
        self.handed.dedup_by_key(|entry| (entry.to, entry.order));

        let mut begins = 0;
        self.handed.chunk_by(|one, next| one.to == next.to).map(move |run| {
            let range = begins..begins + run.len();
            begins = range.end;

            (run[0].to, range)
        })
    }

    /// The items handed out at the start of the step that lie in `range`.
    #[must_use]
    pub fn handed(&self, range: Range<usize>) -> impl ExactSizeIterator<Item = &Pending> {
        self.handed[range].iter().map(|entry| &entry.pending)
    }

    /// Whether any item has arrived since the step began.
    #[must_use]
    pub fn has_items(&self) -> bool {
        !self.arrived.is_empty()
    }
}

impl Pending {
    /// The actor the item comes from.
    pub fn source(&self) -> ActorId {
        match self {
            Self::Mail(envelope) => envelope.id.sender,
            Self::Ended { ended, .. } => ended.actor().id(),
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
            Record::Item(order, pending) => shard.arrived.push(Entry { to: key, order, pending }),
            Record::Watch { by, context } => shard.watches.push((key, by, context)),
            Record::Ended { how, context } => shard.endings.push((key, how, context)),
        }
    }

    /// Pairs the step's watches and endings, each under the actor watched, and hands on an ending to each watcher.
    fn settle(shard: &mut Shard, forward: &mut Vec<(u64, Record)>) {
        // A watch of an actor that ended before this step is shown so, with how it ended no longer kept.
        for (watched, by, context) in shard.watches.drain(..) {
            let entry = shard.watched.entry(watched).or_default();
            let is_over = entry.life == Watching::Over;
            if is_over {
                forward.push(handed(by, watched, How::Earlier, context));
            }
            entry.by.insert(by);
        }

        // An ending of this step goes to every watcher, those whose watch came in this step among them.
        for (ended, how, context) in shard.endings.drain(..) {
            let entry = shard.watched.entry(ended).or_default();
            entry.life = Watching::Over;
            forward.extend(entry.by.iter().map(|watcher| handed(*watcher, ended, how.clone(), context)));
        }
    }
}

impl Consumes for Streams {
    fn takes_owned(effect: Effect, context: &Context) -> impl Iterator<Item = (u64, Record)> + '_ {
        // At most one record, and a mail's bytes are moved into its envelope.
        let by = effect.by();
        let record = match effect.into_what() {
            What::Send { to, id, kind, bytes } => {
                let envelope =
                    Envelope { id, to, cause: context.cause, root: context.root, depth: context.depth, kind, bytes };

                Some((to.bits(), Record::Item((by.bits(), id.count), Pending::Mail(envelope))))
            }
            What::Watch { of } => Some((of.id().bits(), Record::Watch { by: by.bits(), context: *context })),
            What::Ended(how) => Some((by.bits(), Record::Ended { how, context: *context })),
            _ => None,
        };

        record.into_iter()
    }
}

/// The ending of the actor whose bits are `ended`, `how` it ended, as an item for the watcher whose bits are `to`:
/// last of what that actor caused.
fn handed(to: u64, ended: u64, how: How, context: Context) -> (u64, Record) {
    let ended = Ended::new(ActorId::new(ended), how);

    (to, Record::Item((ended.actor().id().bits(), u64::MAX), Pending::Ended { ended, context }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aethereal_actor::MailId;

    /// How each ending handed on goes, by the watcher it goes to.
    fn handed_on(forward: Vec<(u64, Record)>) -> Vec<(u64, How)> {
        forward
            .into_iter()
            .filter_map(|(to, record)| match record {
                Record::Item(_, Pending::Ended { ended, .. }) => Some((to, ended.how().clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn watch_and_ending_of_one_step_pair_in_either_order() {
        // Catches a pairing made record by record, where an ending folded before a watch of the same step leaves
        // that watcher shown `Earlier`, and an ending kept from one step that shows a later watch how it went.
        let context = Context { cause: None, root: MailId { sender: ActorId::new(7), count: 0 }, depth: 1 };
        let mut shard = Shard::default();
        Streams::fold(&mut shard, 7, Record::Ended { how: How::Closed, context });
        Streams::fold(&mut shard, 7, Record::Watch { by: 9, context });
        let mut forward = Vec::new();
        Streams::settle(&mut shard, &mut forward);
        assert_eq!(handed_on(forward), [(9, How::Closed)]);

        Streams::fold(&mut shard, 7, Record::Watch { by: 11, context });
        let mut forward = Vec::new();
        Streams::settle(&mut shard, &mut forward);
        assert_eq!(handed_on(forward), [(11, How::Earlier)]);
    }
}
