//! The view of what each actor has to react to (ADR-0009, ADR-0011).

use crate::id::child_id;
use crate::watch::{Watched, Watchers};
use crate::{ActorTable, Consumes, Context, Envelope, Folded, Standing, Tie, View};
use aethereal_actor::{Effect, Ended, How, To, What};
use aethereal_data::{ActorId, PathSegment};
use std::collections::BTreeMap;
use std::mem;

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
    type Shard = BTreeMap<u64, Vec<(Order, Pending)>>;
    type Record = (Order, Pending);
    type Reads<'a> = (&'a ActorTable, &'a Folded<Watchers>);

    fn takes<'a>(
        effect: &'a Effect,
        context: &'a Context,
        reads: Self::Reads<'a>,
    ) -> impl Iterator<Item = (u64, (Order, Pending))> + 'a {
        Self::takes_owned(effect.clone(), context, reads)
    }

    /// Items are kept as they arrive and put in order when they are taken, by [`take`].
    fn fold(shard: &mut BTreeMap<u64, Vec<(Order, Pending)>>, key: u64, record: (Order, Pending)) {
        shard.entry(key).or_default().push(record);
    }
}

impl Consumes for Streams {
    fn takes_owned<'a>(
        effect: Effect,
        context: &'a Context,
        (table, watchers): Self::Reads<'a>,
    ) -> impl Iterator<Item = (u64, (Order, Pending))> + 'a {
        // At most one record of its own, and a fan-out for an ending: nothing is collected, and a mail's bytes are
        // moved into its envelope.
        let by = effect.by();
        let mut one = None;
        let mut fan = None;
        match effect.into_what() {
            What::Send { to, id, kind, bytes } => {
                let receiver = receiver(table, by, &to).filter(|to| table.standing(*to) == Standing::Live);
                one = receiver.map(|to| {
                    let envelope =
                        Envelope { id, to, cause: context.cause, root: context.root, tie: Tie::Tell, kind, bytes };

                    (to.bits(), ((by.bits(), id.count), Pending::Mail(envelope)))
                });
            }
            What::Watch { of } => {
                let watched = of.id();
                let over = watchers.shard(watched.bits()).get(&watched.bits()).is_some_and(Watched::is_over);
                let earlier = || Pending::Ended { ended: Ended::new(watched, How::Earlier), context: *context };
                one = over.then(|| (by.bits(), ((watched.bits(), u64::MAX), earlier())));
            }
            What::Ended(how) => {
                fan = watchers.shard(by.bits()).get(&by.bits()).map(|watched| {
                    watched.by().iter().map(move |watcher| {
                        let ended = Pending::Ended { ended: Ended::new(by, how.clone()), context: *context };

                        (*watcher, ((by.bits(), u64::MAX), ended))
                    })
                });
            }
            _ => {}
        }

        one.into_iter().chain(fan.into_iter().flatten())
    }
}

/// An actor's items as the view keeps them, put in the order the actor is handed them.
///
/// The order is by source, then the order that source caused each item, so it does not depend on the order the
/// items arrived in. An ending that arrived twice, from the ending and from a watch asked for in the same step,
/// is handed once, keeping how the actor ended.
pub fn take(mut items: Vec<(Order, Pending)>) -> impl Iterator<Item = Pending> {
    items.sort_by_key(|(order, _)| *order);
    items.dedup_by(|later, kept| {
        let is_twice = later.0 == kept.0;
        let keeps_later = kept.1.is_earlier();
        if is_twice && keeps_later {
            mem::swap(later, kept);
        }

        is_twice
    });

    items.into_iter().map(|(_, pending)| pending)
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
