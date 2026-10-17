//! The view of what each actor has to react to (ADR-0009, ADR-0011).

use crate::watch::Notice;
use crate::{Context, Envelope, View};
use aethereal_actor::{Arena, CARRIED, Effect, Ended, MailId, Mailed, Payload, SeatNumber, Seated};
use aethereal_data::ActorId;
use std::iter;
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
    /// How many items the part has been handed in a step of late: the most in one step, falling by an eighth with
    /// each step it is handed fewer, and forgotten once it has been idle [`IDLE_STEPS`] steps in a row. What the
    /// part's lists keep room for once emptied follows from it.
    recent: usize,
    /// How many steps in a row the part has had nothing to run.
    idle: u32,
    /// The items the part's actors were not handed in the step they were handed out, carried over, each actor's in
    /// order: with the endings among them and the bytes of their mail, copied out of the writers' arenas once.
    backlog: Vec<(u64, Entry)>,
    backlog_endings: Vec<(Ended, Context)>,
    backlog_bytes: Vec<u8>,
    /// The runs of the backlog to hand out at the next step, by seat and actor, each actor's in the order carried.
    segments: Vec<Segment>,
    /// The runs of the backlog handed out at the start of this step.
    handing: Vec<Segment>,
}

/// A run of one actor's items in a part's backlog.
#[derive(Clone)]
struct Segment {
    seat: u64,
    to: u64,
    range: Range<usize>,
}

/// One actor's items handed out at the start of a step: the number of the seat it sat in when they were sent, its
/// bits, the runs of the backlog it carried over, which come first, and where what arrived for it lies.
#[derive(Clone, Copy)]
pub struct Run {
    pub seat: u64,
    pub bits: u64,
    carried: Span,
    fresh: Span,
}

/// Where a run of entries lies in a list of them: a range in eight bytes, as a part holds fewer than `u32::MAX`.
#[derive(Clone, Copy)]
struct Span {
    start: u32,
    end: u32,
}

impl Span {
    /// The entries from `range.start` to `range.end`.
    fn of(range: Range<usize>) -> Self {
        let to = |index: usize| u32::try_from(index).unwrap_or(u32::MAX);

        Self { start: to(range.start), end: to(range.end) }
    }

    fn range(self) -> Range<usize> {
        self.start as usize..self.end as usize
    }

    const fn is_empty(self) -> bool {
        self.start == self.end
    }
}

/// The bytes `payload` says where to find: in the arenas of the writers, `mail`, or in `carried`, the bytes a part
/// carried over.
#[must_use]
pub fn read<'a>(payload: Payload, mail: &'a [Arena], carried: &'a [u8]) -> &'a [u8] {
    // No writer has the carried slot, so mail that was carried is not found among the arenas.
    mail.get(payload.slot()).and_then(|arena| arena.read(payload)).unwrap_or_else(|| read_carried(payload, carried))
}

/// `payload`, carried, as it lies from `start` of its part's carried bytes.
const fn carried_from(payload: Payload, start: u32) -> Payload {
    match payload {
        Payload::InArena { len, pieces, .. } => Payload::InArena { slot: CARRIED, start, len, pieces },
        Payload::Spilled { .. } => payload,
    }
}

/// The bytes of `payload` among `carried`, a part's carried bytes, when it was carried: none otherwise.
#[cold]
fn read_carried(payload: Payload, carried: &[u8]) -> &[u8] {
    match payload {
        Payload::InArena { slot: CARRIED, start, len, .. } => {
            let start = start as usize;

            carried.get(start..start + len as usize).unwrap_or_default()
        }
        _ => &[],
    }
}

/// How many steps in a row a part is idle before it gives back the room its lists keep for what it was handed.
pub const IDLE_STEPS: u32 = 4;

/// The fewest items a list of a part's stream keeps room for once it is emptied.
const KEPT_LEAST: usize = 256;

/// Empties nothing, and gives back what room `list`, an empty list, has past `room`.
fn trim<T>(list: &mut Vec<T>, room: usize) {
    let is_spare = list.is_empty() && list.capacity() > room;
    if is_spare {
        list.shrink_to(room);
    }
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
    /// A piece of mail, and its id: of what `envelope` holds, which is one but for a run, the piece whose bytes are
    /// `bytes`.
    Mail { id: MailId, envelope: &'a Envelope, bytes: Payload },
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
    /// at the last step is dropped here. An actor's runs of what it carried over come before what arrived for it,
    /// as they are older than anything from the same source that arrived since.
    pub fn hand(&mut self, runs: &mut Vec<Run>) {
        mem::swap(&mut self.segments, &mut self.handing);
        self.segments.clear();
        // Hops carried after an actor was cut off add runs behind those of the actors after it: a stable sort keeps
        // each actor's in order.
        self.handing.sort_by_key(|segment| (segment.seat, segment.to));
        let is_drained = self.handing.is_empty();
        if is_drained {
            self.backlog.clear();
            self.backlog_endings.clear();
            self.backlog_bytes.clear();
        }
        // What was handed out of the backlog stays in it until it is drained, or until it is most of it.
        let live: usize = self.handing.iter().map(|segment| segment.range.len()).sum();
        let spent = self.backlog.len() - live;
        let is_sparse = spent > live && spent > KEPT_LEAST;
        if is_sparse {
            self.compact();
        }
        self.recent = self.arrived.len().max(self.recent - self.recent / 8);
        self.idle = 0;
        self.handed.clear();
        self.handed_endings.clear();
        mem::swap(&mut self.arrived, &mut self.handed);
        mem::swap(&mut self.arrived_endings, &mut self.handed_endings);
        // What was handed out at the last step is the list items now arrive in.
        let room = self.room();
        trim(&mut self.arrived, room);
        self.handed.sort_unstable_by_key(|(seat, entry)| (*seat, entry.to, entry.order));
        self.handed.dedup_by_key(|(seat, entry)| (*seat, entry.to, entry.order));

        let mut begins = 0;
        let mut fresh = self
            .handed
            .chunk_by(|(seat, one), (next_seat, next)| (*seat, one.to) == (*next_seat, next.to))
            .map(|run| {
                let range = begins..begins + run.len();
                begins = range.end;

                (run[0].0, run[0].1.to, range)
            })
            .peekable();
        // Most steps carry nothing over, and the runs are what arrived.
        if self.handing.is_empty() {
            runs.extend(fresh.map(|(seat, bits, fresh)| Run {
                seat,
                bits,
                carried: Span::of(0..0),
                fresh: Span::of(fresh),
            }));
            return;
        }
        let mut segment = 0;
        loop {
            let carried_key = self.handing.get(segment).map(|run| (run.seat, run.to));
            let fresh_key = fresh.peek().map(|(seat, bits, _)| (*seat, *bits));
            let key = match (carried_key, fresh_key) {
                (None, None) => break,
                (Some(carried), Some(fresh)) => carried.min(fresh),
                (Some(key), None) | (None, Some(key)) => key,
            };
            let first = segment;
            while self.is_carried_for(segment, key) {
                segment += 1;
            }
            let arrived = fresh.next_if(|(seat, bits, _)| (*seat, *bits) == key).map_or(0..0, |(_, _, range)| range);
            runs.push(Run { seat: key.0, bits: key.1, carried: Span::of(first..segment), fresh: Span::of(arrived) });
        }
    }

    /// Whether the carried segment `segment` is there, and of the actor `key` names, by its seat and its bits.
    fn is_carried_for(&self, segment: usize, key: (u64, u64)) -> bool {
        self.handing.get(segment).is_some_and(|run| (run.seat, run.to) == key)
    }

    /// Moves what the backlog still holds to the front of its lists, in order, and drops what was handed out of it.
    fn compact(&mut self) {
        let Self { backlog, backlog_endings, backlog_bytes, handing, .. } = self;
        let mut entries = Vec::new();
        let mut endings = Vec::new();
        let mut bytes = Vec::new();
        for segment in handing.iter_mut() {
            let begins = entries.len();
            for &(seat, mut entry) in &backlog[segment.range.clone()] {
                entry.item = match entry.item {
                    Stored::Mail(mut envelope) => {
                        let start = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
                        bytes.extend_from_slice(read_carried(envelope.bytes, backlog_bytes));
                        envelope.bytes = carried_from(envelope.bytes, start);

                        Stored::Mail(envelope)
                    }
                    Stored::Ended(place) => {
                        endings.push(backlog_endings[place].clone());

                        Stored::Ended(endings.len() - 1)
                    }
                };
                entries.push((seat, entry));
            }
            segment.range = begins..entries.len();
        }
        *backlog = entries;
        *backlog_endings = endings;
        *backlog_bytes = bytes;
    }

    /// The items of `run`, in order: what it carried over, then what arrived for it, a run of mail one piece after
    /// another.
    #[allow(
        clippy::inline_always,
        reason = "once for every actor that runs: a call here cost 5 percent of a turn, measured"
    )]
    #[inline(always)]
    pub fn items<'a>(&'a self, run: &Run) -> Pieces<'a> {
        // Most runs carried nothing, and are what arrived.
        let carries = !run.carried.is_empty();
        let mut pieces = Pieces {
            entries: &self.handed[run.fresh.range()],
            endings: &self.handed_endings,
            at: 0,
            piece: 0,
            each: 0,
            shard: self,
            segments: run.carried.range(),
            fresh: run.fresh.range(),
            on_fresh: !carries,
        };
        if carries {
            pieces.advance();
        }

        pieces
    }

    /// The bytes of the mail carried over.
    #[must_use]
    pub fn carried(&self) -> &[u8] {
        &self.backlog_bytes
    }

    /// Carries over into the next step the items of `run` but the first `taken`, which its actor was not handed in
    /// this step. What it carried already stays where it is; what arrived for it is copied into the backlog, its
    /// bytes out of `mail`, the writers' arenas, which later steps write on.
    pub fn carry(&mut self, run: &Run, mut taken: u32, mail: &[Arena]) {
        for index in run.carried.range() {
            let Segment { seat, to, range } = self.handing[index].clone();
            let mut start = range.start;
            loop {
                let is_left = start < range.end;
                let owes = taken > 0;
                let goes_on = is_left && owes;
                if !goes_on {
                    break;
                }
                let entry = &mut self.backlog[start].1;
                let pieces = entry.pieces();
                if taken >= pieces {
                    taken -= pieces;
                    start += 1;
                    continue;
                }
                if let Stored::Mail(envelope) = &mut entry.item {
                    envelope.bytes = envelope.bytes.from_piece(taken);
                }
                entry.order.1 += u64::from(taken);
                taken = 0;
            }
            if start < range.end {
                self.segments.push(Segment { seat, to, range: start..range.end });
            }
        }

        let begins = self.backlog.len();
        for index in run.fresh.range() {
            let (seat, entry) = self.handed[index];
            let pieces = entry.pieces();
            if taken >= pieces {
                taken -= pieces;
                continue;
            }
            let item = match entry.item {
                Stored::Mail(envelope) => {
                    let rest = envelope.bytes.from_piece(taken);
                    let bytes = read(rest, mail, &[]);
                    let (Ok(start), Ok(len)) = (u32::try_from(self.backlog_bytes.len()), u32::try_from(bytes.len()))
                    else {
                        panic!("a part carries less than u32::MAX bytes of mail");
                    };
                    self.backlog_bytes.extend_from_slice(bytes);
                    let left = u16::try_from(rest.pieces()).unwrap_or(u16::MAX);

                    Stored::Mail(Envelope {
                        bytes: Payload::InArena { slot: CARRIED, start, len, pieces: left },
                        ..envelope
                    })
                }
                Stored::Ended(place) => {
                    self.backlog_endings.push(self.handed_endings[place].clone());

                    Stored::Ended(self.backlog_endings.len() - 1)
                }
            };
            let (sender, count) = entry.order;
            let order = (sender, count + u64::from(taken));
            taken = 0;
            self.backlog.push((seat, Entry { to: entry.to, order, item }));
        }
        if self.backlog.len() > begins {
            self.segments.push(Segment { seat: run.seat, to: run.bits, range: begins..self.backlog.len() });
        }
    }

    /// Carries over into the next step a hop to `to`, an actor cut off in this step, after what it carries already:
    /// the mail of id `order` that `envelope` makes of where its bytes are carried, whose bytes are `bytes`.
    pub fn carry_hop(&mut self, to: Seated, order: Order, envelope: impl FnOnce(Payload) -> Envelope, bytes: &[u8]) {
        let (Ok(start), Ok(len)) = (u32::try_from(self.backlog_bytes.len()), u32::try_from(bytes.len())) else {
            panic!("a part carries less than u32::MAX bytes of mail");
        };
        self.backlog_bytes.extend_from_slice(bytes);
        let bytes = Payload::InArena { slot: CARRIED, start, len, pieces: 1 };
        let entry = Entry { to: to.id.bits(), order, item: Stored::Mail(envelope(bytes)) };
        let seat = to.seat.number();
        let at = self.backlog.len();
        self.backlog.push((seat, entry));
        let goes_on =
            self.segments.last().is_some_and(|last| (last.seat, last.to) == (seat, entry.to) && last.range.end == at);
        if goes_on {
            if let Some(last) = self.segments.last_mut() {
                last.range.end = at + 1;
            }
        } else {
            self.segments.push(Segment { seat, to: entry.to, range: at..at + 1 });
        }
    }

    /// What the backlog holds for the next step, as the plan weighs it: each actor's first carried item, and how
    /// many items of its runs carried, at most `most`.
    pub fn backlog(&self, most: u64) -> impl Iterator<Item = (Seated, Handed<'_>, u64)> {
        self.segments.iter().map(move |segment| {
            let (seat, entry) = &self.backlog[segment.range.start];
            let to = Seated { id: ActorId::new(entry.to), seat: SeatNumber::new(*seat) };
            let mut pieces = 0;
            for (_, entry) in &self.backlog[segment.range.clone()] {
                pieces += u64::from(entry.pieces());
                if pieces >= most {
                    break;
                }
            }

            (to, entry.handed(&self.backlog_endings), pieces.min(most))
        })
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

    /// How many items an emptied list of the part keeps room for: twice what it has been handed in a step of late.
    #[must_use]
    pub fn room(&self) -> usize {
        self.recent.saturating_mul(2).max(KEPT_LEAST)
    }

    /// Counts a step in which the part had nothing to run, and, once it has been idle [`IDLE_STEPS`] steps in a row,
    /// gives back its lists' room past the least: what it was handed in the last step it ran is no longer read.
    /// Answers whether it gave back room, for the engine to trim the buckets the writers keep for it as well.
    pub fn rest(&mut self) -> bool {
        self.idle = self.idle.saturating_add(1);
        let forgets = self.idle == IDLE_STEPS;
        if !forgets {
            return false;
        }
        self.recent = 0;
        self.handed.clear();
        self.handed_endings.clear();
        trim(&mut self.handed, KEPT_LEAST);
        trim(&mut self.arrived, KEPT_LEAST);
        // An idle part carries nothing, as a part with items to run is never idle, so all its backlog holds was
        // handed in the last step it ran.
        self.handing.clear();
        self.backlog.clear();
        self.backlog_endings.clear();
        self.backlog_bytes.clear();
        trim(&mut self.handing, KEPT_LEAST);
        trim(&mut self.backlog, KEPT_LEAST);
        trim(&mut self.backlog_endings, KEPT_LEAST);
        trim(&mut self.backlog_bytes, KEPT_LEAST);

        true
    }

    /// Whether any item has arrived since the step began, or is carried over into the next.
    #[must_use]
    pub fn has_items(&self) -> bool {
        !self.arrived.is_empty() || !self.segments.is_empty()
    }
}

/// The items of a range of a part's stream, a run of mail one piece after another.
pub struct Pieces<'a> {
    entries: &'a [(u64, Entry)],
    endings: &'a [(Ended, Context)],
    /// The entry the next item is of, which of its pieces, and how many bytes each of them is.
    at: usize,
    piece: u32,
    each: u32,
    /// The part the items are of, the runs of its backlog still to hand out after `entries`, and where what arrived
    /// lies, unless `entries` are that already.
    shard: &'a Shard,
    segments: Range<usize>,
    fresh: Range<usize>,
    on_fresh: bool,
}

impl Pieces<'_> {
    /// Moves on to the next run of entries: the next carried, then what arrived. Answers whether there was one.
    #[cold]
    #[inline(never)]
    fn advance(&mut self) -> bool {
        self.at = 0;
        let shard = self.shard;
        if let Some(segment) = self.segments.next() {
            self.entries = &shard.backlog[shard.handing[segment].range.clone()];
            self.endings = &shard.backlog_endings;
            return true;
        }
        if self.on_fresh {
            return false;
        }
        self.entries = &shard.handed[self.fresh.clone()];
        self.endings = &shard.handed_endings;
        self.on_fresh = true;

        true
    }
}

impl<'a> Iterator for Pieces<'a> {
    type Item = Handed<'a>;

    fn next(&mut self) -> Option<Handed<'a>> {
        let (_, entry) = loop {
            match self.entries.get(self.at) {
                Some(found) => break found,
                None if self.advance() => {}
                None => return None,
            }
        };
        let first = entry.handed(self.endings);
        let pieces = entry.pieces();
        if pieces == 1 {
            self.at += 1;
            return Some(first);
        }
        if self.piece == 0 {
            self.each = first.each();
        }
        let item = first.piece(self.piece, self.each);
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

    /// The item as a turn reads it, its ending found in `endings` when it is one.
    fn handed<'a>(&'a self, endings: &'a [(Ended, Context)]) -> Handed<'a> {
        match &self.item {
            Stored::Mail(envelope) => {
                let (sender, count) = self.order;

                Handed::Mail { id: MailId { sender: ActorId::new(sender), count }, envelope, bytes: envelope.bytes }
            }
            Stored::Ended(place) => {
                let (ended, context) = &endings[*place];

                Handed::Ended { ended, context }
            }
        }
    }
}

impl Handed<'_> {
    /// The piece numbered `index` of a run this is the first piece of, each of its pieces `each` bytes.
    #[must_use]
    pub fn piece(self, index: u32, each: u32) -> Self {
        match self {
            Self::Mail { id, envelope, .. } if index > 0 => {
                let id = MailId { sender: id.sender, count: id.count + u64::from(index) };

                Self::Mail { id, envelope, bytes: envelope.bytes.piece(index, each) }
            }
            Self::Mail { id, envelope, .. } => Self::Mail { id, envelope, bytes: envelope.bytes.piece(0, each) },
            Self::Ended { .. } => self,
        }
    }

    /// How many bytes each piece of a run this is the first piece of is.
    #[must_use]
    pub fn each(self) -> u32 {
        match self {
            Self::Mail { envelope, .. } => envelope.bytes.each(),
            Self::Ended { .. } => 0,
        }
    }

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

    /// Mail comes in its own buffer, by [`entry`], and no effect is mail.
    fn takes<'a>(_: &'a Effect, _: &'a Context) -> impl Iterator<Item = (u64, Entry)> + 'a {
        iter::empty()
    }

    /// Items are kept as they arrive and put in order when they are handed out, by [`Shard::hand`].
    fn fold(shard: &mut Shard, key: u64, record: Entry, _: &[Arena]) {
        shard.arrived.push((key, record));
    }

    /// A writer's bucket is taken whole: in place of the part's list when nothing has arrived yet and the list has
    /// no more room than the bucket, or added to its end. The bucket is left empty, with room, and never with more
    /// than it had, so a writer's buckets keep only the room its own records took.
    fn fold_all(shard: &mut Shard, records: &mut Vec<(u64, Entry)>, _: &[Arena]) {
        let takes_place = shard.arrived.is_empty() && shard.arrived.capacity() <= records.capacity();
        if takes_place {
            mem::swap(&mut shard.arrived, records);
        } else {
            shard.arrived.append(records);
        }
        trim(records, shard.room());
    }
}

/// The key and entry the streams keep of `mailed`, sent in `context`: its receiver's seat, and the mail in its
/// envelope.
#[must_use]
pub fn entry(mailed: Mailed, context: &Context) -> (u64, Entry) {
    let Mailed { by, to, count, kind, bytes } = mailed;
    let envelope = Envelope { sender_seat: by.seat, root: context.root, depth: context.depth, kind, bytes };
    let entry = Entry { to: to.id.bits(), order: (by.id.bits(), count), item: Stored::Mail(envelope) };

    (to.seat.number(), entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A part handed `items` items in a step, all for one actor.
    fn handed(items: u64) -> Shard {
        let mut shard = Shard::default();
        for count in 0..items {
            shard.arrived.push((0, Entry { to: 1, order: (2, count), item: Stored::Ended(0) }));
        }
        let mut runs = Vec::new();
        shard.hand(&mut runs);
        assert_eq!(runs.len(), 1);

        shard
    }

    #[test]
    fn idle_part_gives_back_the_room_of_a_burst_and_a_busy_one_keeps_it() {
        // Catches a part that keeps a burst's room while it idles, so each part an engine ever ran a burst in holds
        // it for good, and one that gives it back while still busy, so each burst grows its lists again.
        let mut idle = handed(10_000);
        for _ in 1..IDLE_STEPS {
            assert!(!idle.rest());
        }
        assert!(idle.handed.capacity() >= 10_000, "a part idle fewer steps keeps its room");
        assert!(idle.rest());
        assert!(idle.handed.capacity() <= KEPT_LEAST);
        assert!(idle.arrived.capacity() <= KEPT_LEAST);

        let mut busy = handed(10_000);
        for _ in 1..IDLE_STEPS {
            assert!(!busy.rest());
        }
        busy.arrived.push((0, Entry { to: 1, order: (2, 0), item: Stored::Ended(0) }));
        busy.hand(&mut Vec::new());
        for _ in 1..IDLE_STEPS {
            assert!(!busy.rest());
        }
        assert!(busy.room() >= 10_000, "a part handed items is idle again from none");
    }

    #[test]
    fn idle_part_gives_back_a_backlog_it_handed_in_full() {
        // Catches a part that keeps what it carried once it is all handed: the backlog is cleared when a part is next
        // handed, and an idle part is never handed again until mail reaches it.
        let mut part = handed(1);
        part.backlog.extend((0..10_000).map(|count| (0, Entry { to: 1, order: (2, count), item: Stored::Ended(0) })));
        part.backlog_bytes.resize(10_000, 0);
        part.handing.push(Segment { seat: 0, to: 1, range: 0..10_000 });
        assert!(!part.has_items());

        for _ in 0..IDLE_STEPS {
            part.rest();
        }

        assert!(part.backlog.capacity() <= KEPT_LEAST);
        assert!(part.backlog_bytes.capacity() <= KEPT_LEAST);
        assert!(part.handing.is_empty());
    }
}
