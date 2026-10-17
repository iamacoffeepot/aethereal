//! The view of what each actor has to react to (ADR-0009, ADR-0011).

use crate::watch::Notice;
use crate::{Consumes, Context, Envelope, View};
use aethereal_actor::{Effect, Ended, MailId, SeatNumber, Seated, What};
use aethereal_data::ActorId;
use std::mem;
use std::ops::Range;

/// The view of what each actor has to react to: its stream of items (ADR-0009).
///
/// It reads no other view. Mail is kept under the seat of the actor its turn
/// named, whether or not that actor is still live: an actor that is not live
/// is never handed its items, and they are dropped there. The seat comes with
/// the actor's id wherever an actor is named, so the items of a step are handed
/// out in the order of the seats. The endings of the actors an actor watches
/// come from the watches, [`crate::watch::Watches`], once a step settles.
///
/// Many actors write one key, so an actor's items are kept in an order that
/// does not depend on the order they arrive in: by their source, then in the
/// order that source caused them. An actor's ending is the last thing it
/// causes, so it comes after that actor's mail.
///
/// A part keeps the items of all its actors in one list, as they arrive. A
/// writer's records for the part are entries of that list already, so the part
/// takes a writer's bucket whole. At the start of a step [`Shard::hand`] puts
/// the list in order, once, and the step's turns read each actor's run of it
/// where it lies. An entry in the list owns nothing: an ending, which holds how
/// its actor ended, is kept in a list of its own beside it, so the list is
/// emptied without a look at what it held.
pub struct Streams;

/// One part of the view.
#[derive(Default)]
pub struct Shard {
    /// The items that arrived since the step began, for whichever actor, as they arrived, under the number of the
    /// seat of the actor each is for.
    arrived: Vec<(u64, Entry)>,
    /// The endings among the items that arrived since the step began, which their entries number.
    arrived_endings: Vec<(Ended, Context)>,
    /// The items handed out at the start of the step, in order: by seat and actor, then as each actor is handed them.
    handed: Vec<(u64, Entry)>,
    /// The endings among the items handed out at the start of the step, which their entries number.
    handed_endings: Vec<(Ended, Context)>,
}

/// Where an item stands in its stream: its source's bits, then the order that source caused it in.
pub type Order = (u64, u64);

/// One item in a part, and the actor it is for: what the view keeps of a piece of mail, under the number of the
/// seat of the actor it is for.
///
/// Only the view makes one: of a piece of mail as it is sent, or of an ending in the part that keeps it.
#[derive(Clone, Copy)]
pub struct Entry {
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
    /// A piece of mail, and its id: the piece numbered `piece` of what `envelope` holds, which is one but for a run.
    Mail { id: MailId, envelope: &'a Envelope, piece: u32 },
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
        self.handed.sort_unstable_by_key(|(seat, entry)| (*seat, entry.to, entry.order));
        self.handed.dedup_by_key(|(seat, entry)| (*seat, entry.to, entry.order));

        let mut begins = 0;
        self.handed.chunk_by(|(seat, one), (next_seat, next)| (*seat, one.to) == (*next_seat, next.to)).map(
            move |run| {
                let range = begins..begins + run.len();
                begins = range.end;

                (run[0].0, run[0].1.to, range)
            },
        )
    }

    /// The items handed out at the start of the step that lie in `range`, a run of mail one piece after another.
    pub fn handed(&self, range: Range<usize>) -> Pieces<'_> {
        Pieces { entries: &self.handed[range], endings: &self.handed_endings, at: 0, piece: 0 }
    }

    /// The items that arrived since the step began, in no order: the actor each is for, the item, the first of a
    /// run, and how many pieces it holds.
    pub fn arrived(&self) -> impl Iterator<Item = (Seated, Handed<'_>, u32)> {
        self.arrived.iter().map(|(seat, entry)| {
            let to = Seated { id: ActorId::new(entry.to), seat: SeatNumber::new(*seat) };

            (to, entry.handed(&self.arrived_endings), entry.pieces())
        })
    }

    /// Puts an ending handed on by the watches in the stream of the watcher, last of what the actor that ended
    /// caused.
    pub fn end(&mut self, notice: Notice) {
        let Notice { to, ended, context } = notice;
        let order = (ended.actor().seated().id.bits(), u64::MAX);
        self.arrived_endings.push((ended, context));
        let item = Stored::Ended(self.arrived_endings.len() - 1);
        self.arrived.push((to.seat.number(), Entry { to: to.id.bits(), order, item }));
    }

    /// Whether any item has arrived since the step began.
    #[must_use]
    pub fn has_items(&self) -> bool {
        !self.arrived.is_empty()
    }
}

/// The items of a range of a part's stream, a run of mail one piece after another.
pub struct Pieces<'a> {
    entries: &'a [(u64, Entry)],
    endings: &'a [(Ended, Context)],
    /// The entry the next item is of, and which of its pieces.
    at: usize,
    piece: u32,
}

impl<'a> Iterator for Pieces<'a> {
    type Item = Handed<'a>;

    fn next(&mut self) -> Option<Handed<'a>> {
        let (_, entry) = self.entries.get(self.at)?;
        let Stored::Mail(envelope) = &entry.item else {
            self.at += 1;
            return Some(entry.handed(self.endings));
        };
        let pieces = envelope.bytes.pieces();
        if pieces == 1 {
            self.at += 1;
            return Some(entry.handed(self.endings));
        }
        let item = entry.piece(self.piece, self.endings);
        self.piece += 1;
        if self.piece == pieces {
            self.at += 1;
            self.piece = 0;
        }

        Some(item)
    }
}

impl Entry {
    /// How many items it holds: one, but for a run of mail.
    fn pieces(&self) -> u32 {
        match &self.item {
            Stored::Mail(envelope) => envelope.bytes.pieces(),
            Stored::Ended(_) => 1,
        }
    }

    /// The item numbered `index` it holds, as a turn reads it: the piece of a run of mail, with its own id and bytes.
    fn piece<'a>(&'a self, index: u32, endings: &'a [(Ended, Context)]) -> Handed<'a> {
        match &self.item {
            Stored::Mail(envelope) => {
                let (sender, count) = self.order;
                let id = MailId { sender: ActorId::new(sender), count: count + u64::from(index) };

                Handed::Mail { id, envelope, piece: index }
            }
            Stored::Ended(_) => self.handed(endings),
        }
    }

    /// The item as a turn reads it, its ending found in `endings` when it is one.
    fn handed<'a>(&'a self, endings: &'a [(Ended, Context)]) -> Handed<'a> {
        match &self.item {
            Stored::Mail(envelope) => {
                let (sender, count) = self.order;

                Handed::Mail { id: MailId { sender: ActorId::new(sender), count }, envelope, piece: 0 }
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
            Self::Mail { id, envelope, .. } => Seated { id: id.sender, seat: envelope.sender_seat },
            Self::Ended { ended, .. } => ended.actor().seated(),
        }
    }
}

impl View for Streams {
    type Shard = Shard;
    type Record = Entry;

    fn takes<'a>(effect: &'a Effect, context: &'a Context) -> impl Iterator<Item = (u64, Entry)> + 'a {
        Self::takes_owned(effect.clone(), context)
    }

    /// Items are kept as they arrive and put in order when they are handed out, by [`Shard::hand`].
    fn fold(shard: &mut Shard, key: u64, record: Entry) {
        shard.arrived.push((key, record));
    }

    /// A writer's bucket is taken whole: in place of the part's list when nothing has arrived yet and the list has
    /// no more room than the bucket, or added to its end. The bucket is left empty, with room, and never with more
    /// than it had, so a writer's buckets keep only the room its own records took.
    fn fold_all(shard: &mut Shard, records: &mut Vec<(u64, Entry)>) {
        let takes_place = shard.arrived.is_empty() && shard.arrived.capacity() <= records.capacity();
        if takes_place {
            mem::swap(&mut shard.arrived, records);
        } else {
            shard.arrived.append(records);
        }
    }
}

impl Consumes for Streams {
    fn takes_owned(effect: Effect, context: &Context) -> impl Iterator<Item = (u64, Entry)> + '_ {
        // At most one record, and a mail's bytes are moved into its envelope.
        let by = effect.seated();
        let record = match effect.into_what() {
            What::Send { to, count, kind, bytes } => {
                let (root, depth) = (context.root, context.depth);
                let envelope = Envelope { sender_seat: by.seat, root, depth, kind, bytes };
                let entry = Entry { to: to.id.bits(), order: (by.id.bits(), count), item: Stored::Mail(envelope) };

                Some((to.seat.number(), entry))
            }
            _ => None,
        };

        record.into_iter()
    }
}
