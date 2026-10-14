//! The view of what each actor has to react to (ADR-0009, ADR-0011).

use crate::id::child_id;
use crate::watch::{Watched, Watchers};
use crate::{ActorTable, Context, Envelope, Folded, Standing, Tie, View};
use aethereal_actor::{Effect, Ended, How, To, What};
use aethereal_data::{ActorId, PathSegment};
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

/// The view of what each actor has to react to, keyed by that actor's id: its stream of items (ADR-0009).
///
/// It reads the table, to know who mail is for and whether they are live, and
/// the watchers, to know who an ending goes to. Many actors write one key, so
/// an actor's items are kept in an order that does not depend on the order
/// they arrive in: by their source, then in the order that source caused them.
/// An actor's ending is the last thing it causes, so it comes after that
/// actor's mail.
pub struct Streams;

/// One item in an actor's stream.
pub enum Pending {
    /// A piece of mail.
    Mail(Envelope),
    /// An actor it watches ended. What it does about it belongs to `context`, the chain that ended that actor.
    Ended { ended: Ended, context: Context },
}

/// Where an item stands in its stream: its source's bits, then the order that source caused it in.
pub type Order = (u64, u64);

impl Pending {
    /// The actor the item comes from.
    pub fn source(&self) -> ActorId {
        match self {
            Self::Mail(envelope) => envelope.id.sender,
            Self::Ended { ended, .. } => ended.actor().id(),
        }
    }

    /// Whether this is an ending whose watch was asked for after it, with how the actor ended not kept.
    fn is_earlier(&self) -> bool {
        let Self::Ended { ended, .. } = self else {
            return false;
        };

        *ended.how() == How::Earlier
    }
}

impl View for Streams {
    type Shard = BTreeMap<u64, BTreeMap<Order, Pending>>;
    type Record = (Order, Pending);
    type Reads<'a> = (&'a ActorTable, &'a Folded<Watchers>);

    fn takes<'a>(
        effect: &'a Effect,
        context: &'a Context,
        (table, watchers): Self::Reads<'a>,
    ) -> impl Iterator<Item = (u64, (Order, Pending))> + 'a {
        let by = effect.by();
        let records: Vec<(u64, (Order, Pending))> = match effect.what() {
            What::Send { to, id, kind, bytes } => {
                let receiver = receiver(table, by, to).filter(|to| table.standing(*to) == Standing::Live);
                let envelope = |to: ActorId| Envelope {
                    id: *id,
                    to,
                    cause: context.cause,
                    root: context.root,
                    tie: Tie::Tell,
                    kind: *kind,
                    bytes: bytes.clone(),
                };

                receiver
                    .map(|to| (to.bits(), ((by.bits(), id.count), Pending::Mail(envelope(to)))))
                    .into_iter()
                    .collect()
            }
            What::Ended(how) => {
                let watched = watchers.shard(by.bits()).get(&by.bits());
                let by_watchers = watched.into_iter().flat_map(|watched| watched.by().iter());

                by_watchers
                    .map(|watcher| {
                        let ended = Pending::Ended { ended: Ended::new(by, how.clone()), context: *context };

                        (*watcher, ((by.bits(), u64::MAX), ended))
                    })
                    .collect()
            }
            What::Watch { of } => {
                let watched = of.id();
                let over = watchers.shard(watched.bits()).get(&watched.bits()).is_some_and(Watched::is_over);
                let earlier = Pending::Ended { ended: Ended::new(watched, How::Earlier), context: *context };

                over.then_some((by.bits(), ((watched.bits(), u64::MAX), earlier))).into_iter().collect()
            }
            _ => Vec::new(),
        };

        records.into_iter()
    }

    /// An ending folded twice for one watcher, once from the ending and once from a watch asked for in the same
    /// step, keeps how the actor ended, whichever arrives first.
    fn fold(shard: &mut BTreeMap<u64, BTreeMap<Order, Pending>>, key: u64, (order, pending): (Order, Pending)) {
        match shard.entry(key).or_default().entry(order) {
            Entry::Vacant(free) => {
                free.insert(pending);
            }
            Entry::Occupied(mut held) => {
                let replaces = held.get().is_earlier();
                if replaces {
                    held.insert(pending);
                }
            }
        }
    }
}

/// The actor `to` names for mail the actor `by` sent, or none when it names none.
fn receiver(table: &ActorTable, by: ActorId, to: &To) -> Option<ActorId> {
    match to {
        To::Parent => table.parent(by),
        To::Child(kind) => Some(child_id(by, &PathSegment::new(kind.clone(), &()))),
        To::ChildInstanced(kind, key) => Some(child_id(by, &PathSegment::instanced(kind.clone(), key.clone()))),
        To::Held(reference) => Some(reference.id()),
        // A turn puts the actor it names in its place before the effect is made.
        To::Sender => None,
    }
}
