//! What a guest's code runs on inside its instance: the calls its host makes, which [`guest!`](crate::guest) exports
//! (ADR-0015).
//!
//! A guest runs the same actors a native program does, on the same turns. What
//! a turn reads of the engine, the actor a path names, whether an actor is
//! live, and what it needs to spawn a child, it asks its host for in a call
//! out, which the host answers from what its batch was lent; it checks no
//! send, which its host checks as it reads the turn's mail.
//! After each call the guest writes a record of what each of its turns asked
//! for, in the layout of [`records`](crate::records), and its host reads each
//! where it lies, the bytes of mail and the text of lines among them. A child
//! that sits in the part of the actor that spawned it lives in the same
//! instance; one that sits in another part is taken out at [`Within`], for its
//! host to take into that part's instance. A child of other code, a native
//! type or another blob's, its host begins in a call out, and holds.
//!
//! The guest's references to blobs are the ids native code holds, and their
//! pins are noted in the guest's own buffer: after each call the guest hands
//! its host what the call's pin changes came to for each blob, where each
//! reference lies in what it sent, and the blobs an actor's state taken out
//! refers to, and its host holds the instance to them (ADR-0014). A read of a
//! blob, its hash, and a put are calls out.

use crate::records::{
    self, Born, ClaimRecord, Gone, Handed, ItemHead, Kept, Keying, LineRecord, Naming, Other, OutHead, Parent,
    PinRecord, RefRecord, Requesting, SendRecord, Span, SpawnRecord, Spawning, StoodRecord, Stopped, TurnEnds,
    WatchRecord,
};
use crate::section;
use crate::{
    Above, Actor, Actors, ActorsReader, Asked, Asking, BlobHash, BlobId, Children, Declared, Ended, Fault, How, Item,
    Tie, Tied, Unanswered,
};
use crate::{Beginning, Birth, BlobType, Closing, Guests, Instances, Keeps, Mail, Pinned, Preparing, Puts, Refused};
use crate::{Runs, SeatNumber, Seated, Seats, Table, Turning, Unfit, What, Within, WithinReader, WithinWriter, pins};
use aethereal_data::{
    ActorId, ActorPath, Budget, Decode, DecodeError, Encode, KindId, PathSegment, Reader, Writer, from_bytes, to_bytes,
};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::any::Any;
use core::cell::OnceCell;
use core::cell::{Cell, RefCell};
use core::mem;
use core::slice;

/// Begins an actor of one type as it is seated, where it stands, from its config's bytes.
type Begin = fn(&[u8], Seated, &Standing, &Host, &mut Asked, &mut Instances) -> Result<Box<dyn Runs>, Fault>;

/// Takes an actor of one type in from where it stands and its state, as [`take_out`] wrote them.
type TakeIn = fn(&[u8], usize) -> Option<(Box<dyn Runs>, Standing)>;

/// Where an actor of the guest stands: its path, and what is above it.
struct Standing {
    path: ActorPath,
    above: Above,
}

impl Standing {
    /// Where `born` says an actor stands.
    fn of(born: Born) -> Self {
        let above = match born.parent {
            Parent::Chassis => Above::Chassis,
            Parent::Beneath { id, seat } => {
                Above::Elsewhere(Seated { id: ActorId::new(id), seat: SeatNumber::new(seat) })
            }
        };

        Self { path: born.path, above }
    }

    /// Where it stands, as it is written.
    fn born(&self) -> Born {
        let parent = match self.above {
            Above::Chassis => Parent::Chassis,
            Above::Actor(above, _) | Above::Elsewhere(above) => {
                Parent::Beneath { id: above.id.bits(), seat: above.seat.number() }
            }
        };

        Born { path: self.path.clone(), parent }
    }
}

/// What the guest's code does for one actor type it makes: begin one, take one in, and take one out.
pub struct Type {
    declared: &'static Declared,
    begin: Begin,
    take_in: TakeIn,
    take_out: fn(&dyn Runs, &Standing, &mut Vec<u8>) -> Option<()>,
}

impl Type {
    /// What the guest's code does for actors of type `A`.
    #[must_use]
    pub const fn of<A: Actor>() -> Self {
        Self { declared: Declared::of::<A>(), begin: begin::<A>, take_in: take_in::<A>, take_out: take_out::<A> }
    }
}

/// The calls out a guest makes to its host, which the guest's crate imports: each answered from what the call under
/// way was lent (ADR-0015).
pub struct Calls {
    /// The id of the child the [`Naming`] written at the address, of the length, names, beneath the actor of the id.
    pub child: extern "C" fn(u64, u32, u32) -> u64,
    /// The seat of the child of the id, beneath the actor whose call it is, when it was made or stood up in the step:
    /// `u64::MAX` when none was.
    pub child_seat: extern "C" fn(u64) -> u64,
    /// Whether the actor whose call it is has made the child of the id: one when it has.
    pub has_made: extern "C" fn(u64) -> u32,
    /// Whether the path written at the address, of the length, names a live actor: one when it does, and then its
    /// id and seat are written at the third address, as 16 bytes.
    pub resolve: extern "C" fn(u32, u32, u32) -> u32,
    /// Whether the actor of the id is live: one when it is.
    pub is_live: extern "C" fn(u64) -> u32,
    /// Whether the type of the actor of the id lists every kind of the ids written at the address, of the length:
    /// one when it does.
    pub lists: extern "C" fn(u64, u32, u32) -> u32,
    /// Whether the table held the actor of the id as the step began, or it was stood up since in the part: one when
    /// it was.
    pub held: extern "C" fn(u64) -> u32,
    /// A seat for a child, which [`given_seat`] reads from what the host answers.
    pub take: fn() -> SeatNumber,
    /// Hands back the seat of a child that never began.
    pub give_back: fn(SeatNumber),
    /// Begins the child of other code the [`Spawning`] written at the address, of the length, names: answers as
    /// [`records::refusal`] writes it.
    pub spawn: extern "C" fn(u32, u32) -> u64,
    /// Writes the fault the `begin` of the last child of other code refused returned at the address: its reason's
    /// kind, and then its bytes, of the length the spawn's answer said.
    pub refusal: extern "C" fn(u32),
    /// The actor types the guest's blob makes, which it begins itself: its host begins a child of any other.
    pub made: &'static [&'static Declared],
    /// How many bytes the blob of the engine's number and bits holds, when the instance holds a pin on it or one was
    /// offered to the call: `u64::MAX` otherwise.
    pub blob_len: extern "C" fn(u32, u64) -> u64,
    /// Copies the bytes of the blob of the engine's number and bits to the address, as [`Self::blob_len`] allows.
    pub blob_read: extern "C" fn(u32, u64, u32),
    /// Writes the content hash of the blob of the engine's number and bits at the address, as 32 bytes, as
    /// [`Self::blob_len`] allows: one when it did.
    pub blob_hash: extern "C" fn(u32, u64, u32) -> u32,
    /// Puts the bytes as a blob, in a call out to its host, which [`put_with`] answers.
    pub put: fn(Vec<u8>) -> Result<BlobId, Refused>,
}

/// What the guest holds: its actors, where its host writes what it hands in, and where it writes what it asked for.
#[derive(Default)]
struct Guest {
    actors: Vec<Slot>,
    /// The most bytes a read of one piece of mail, config, or state allocates, as its host sets it as it loads the
    /// instance: none before, so nothing is read.
    read_limit: usize,
    /// The handles of the slots in `actors` that are empty, given to the next actors the guest keeps.
    free: Vec<u32>,
    inbox: Vec<u8>,
    out: Vec<u8>,
    /// The bytes the records point to that no buffer of `asked` holds: the keys of children spawned, and the paths and
    /// state of children taken out.
    held: Vec<u8>,
    /// The pin records of a call that writes no other records, as its host asks for them after it.
    pinned: Vec<u8>,
    /// The pin changes taken from the guest's buffer, emptied and kept for its room.
    changes: Vec<pins::Change>,
    asked: Asked,
    /// What its actors' turns are lent: a guest's actors are its own code, run in no instance, so it holds no
    /// instance, and it carries the calls out to its host that begin a child of any other code.
    instances: Instances,
}

/// One actor of the guest, by the handle its host knows it by.
enum Slot {
    Live {
        runs: Box<dyn Runs>,
        me: Seated,
        standing: Standing,
    },
    /// Empty, as its actor ended or was taken out of the instance: its handle is given to the next actor kept.
    Empty,
}

std::thread_local! {
    static GUEST: RefCell<Guest> = RefCell::new(Guest::new());
    /// The seats the host gave in the call under way that are in another part than the actor whose call it is.
    static ELSEWHERE: RefCell<Vec<SeatNumber>> = const { RefCell::new(Vec::new()) };
    /// Which of the call under way's turns is running, counted from zero.
    static TURN: Cell<u32> = const { Cell::new(0) };
    /// The type of each blob's the guest has spawned from, as it holds the type declares: kept for as long as the
    /// guest runs, as what its own types declare is.
    static BLOB_TYPES: RefCell<Vec<&'static BlobType>> = const { RefCell::new(Vec::new()) };
}

/// The handle of an actor that did not begin, or was not kept in this instance.
const NO_HANDLE: u32 = u32::MAX;

/// The top bit of a seat the host gives, set when the seat is in another part.
const ELSEWHERE_BIT: u64 = 1 << 63;

/// Begins an actor of type `A` as `me`, where it stands, from its config's bytes.
fn begin<A: Actor>(
    config: &[u8],
    me: Seated,
    standing: &Standing,
    host: &Host,
    asked: &mut Asked,
    instances: &mut Instances,
) -> Result<Box<dyn Runs>, Fault> {
    let mut budget = Budget::new(instances.read_limit());
    let mut reader = Reader::<Actors>::new(config, &mut budget, ActorsReader::new());
    let config = A::Config::decode(&mut reader)?;
    let Standing { path, above } = standing;
    // A guest's code keeps nothing (ADR-0010, ADR-0015).
    let mut beginning = Beginning::beneath::<A>(me, path, *above, host, asked, instances, Keeps::Claimed);
    let actor = A::begin(config, &mut beginning)?;

    Ok(Box::new(actor))
}

/// An actor of type `A` taken in from `bytes`, its path and state as [`take_out`] wrote them, read within `read_limit`
/// bytes: nothing when they do not read back.
fn take_in<A: Actor>(bytes: &[u8], read_limit: usize) -> Option<(Box<dyn Runs>, Standing)> {
    let (state, _) = records::carried(bytes)?;
    let mut budget = Budget::new(read_limit);
    let mut reader = Reader::<Within>::new(state, &mut budget, WithinReader::new());
    let born = Born::decode(&mut reader).ok()?;
    let actor = A::decode(&mut reader).ok()?;

    Some((Box::new(actor), Standing::of(born)))
}

/// Writes `runs`, an actor of type `A` that stands where `standing` says, after `out`, as
/// [`records::carried`] reads it: the length of what follows, where it stands and then its state, at [`Within`], and
/// then the blob of each reference in them. Nothing when it is not of type `A`, or does not encode.
fn take_out<A: Actor>(runs: &dyn Runs, standing: &Standing, out: &mut Vec<u8>) -> Option<()> {
    let actor = (runs as &dyn Any).downcast_ref::<A>()?;
    let mut blobs = Vec::new();
    let mut writer = Writer::<Within>::new(WithinWriter::noting(&mut blobs));
    standing.born().encode(&mut writer).ok()?;
    actor.encode(&mut writer).ok()?;
    let state = writer.into_bytes();
    out.extend_from_slice(&count(state.len()).to_le_bytes());
    out.extend_from_slice(&state);
    for blob in blobs {
        out.extend_from_slice(&blob.engine().to_le_bytes());
        out.extend_from_slice(&blob.bits().to_le_bytes());
    }

    Some(())
}

/// Where the host writes up to `len` bytes to hand in: an address in the guest's memory, which stays where it is until
/// the host asks for more room than it has.
#[must_use]
pub fn inbox(len: u32) -> u32 {
    GUEST.with_borrow_mut(|guest| {
        let short = guest.inbox.len() < len as usize;
        if short {
            guest.inbox.resize(len as usize, 0);
        }

        address(guest.inbox.as_ptr())
    })
}

/// Where what the last call asked for lies: its address in the high 32 bits, and its length in the low.
#[must_use]
pub fn out() -> u64 {
    GUEST.with_borrow(Guest::out_at)
}

/// Begins an actor of the `of`th type in `types`, as `me` and `seat` name it, from the first `len` bytes of the inbox:
/// a [`Born`] saying where it stands, its length first, and then its config. Its handle, or `u32::MAX` when its
/// `begin` failed.
#[must_use]
pub fn begin_at(types: &[Type], calls: &'static Calls, of: u32, me: u64, seat: u64, len: u32) -> u32 {
    GUEST.with_borrow_mut(|guest| {
        let me = Seated { id: ActorId::new(me), seat: SeatNumber::new(seat) };
        let Some(made) = types.get(of as usize) else {
            return NO_HANDLE;
        };
        // What the last call asked for has been read by now, so its buffers are emptied for this one.
        guest.open(calls);
        let inbox = mem::take(&mut guest.inbox);
        let len = (len as usize).min(inbox.len());
        let began = standing(&inbox[..len], guest.read_limit).map_err(Fault::from).and_then(|(standing, config)| {
            let Guest { asked, instances, .. } = &mut *guest;
            let runs = (made.begin)(config, me, &standing, &Host::new(calls), asked, instances)?;
            Ok((runs, standing))
        });
        guest.inbox = inbox;
        let (handle, last) = match began {
            Ok((runs, standing)) => (guest.seat(runs, me, standing), Last::Ran),
            Err(fault) => (NO_HANDLE, Last::Failed(fault)),
        };
        guest.out.clear();
        guest.out.resize(OutHead::BYTES, 0);
        write_ends(&mut guest.out, &guest.asked, &mut Counted::default());
        guest.write_records(types, 1, 0, last);

        handle
    })
}

/// Where an actor to begin stands, and its config, from what its host handed in: a [`Born`], its length first, and then
/// the config's bytes. The [`Born`] is read within `read_limit` bytes.
fn standing(handed: &[u8], read_limit: usize) -> Result<(Standing, &[u8]), DecodeError> {
    let (len, rest) = handed.split_at_checked(4).ok_or(DecodeError::UnexpectedEnd)?;
    let len = len.try_into().map_or(0, u32::from_le_bytes) as usize;
    let (born, config) = rest.split_at_checked(len).ok_or(DecodeError::UnexpectedEnd)?;
    let born = from_bytes::<Born>(born, &mut Budget::new(read_limit))?;

    Ok((Standing::of(born), config))
}

/// Takes in an actor of the `of`th type in `types`, as `me` and `seat` name it, from the first `len` bytes of the
/// inbox: its path and state, as the guest that took it out wrote them. Its handle, or `u32::MAX` when they do not read
/// back.
#[must_use]
pub fn take_in_at(types: &[Type], of: u32, me: u64, seat: u64, len: u32) -> u32 {
    GUEST.with_borrow_mut(|guest| {
        let me = Seated { id: ActorId::new(me), seat: SeatNumber::new(seat) };
        let Some(made) = types.get(of as usize) else {
            return NO_HANDLE;
        };
        let len = (len as usize).min(guest.inbox.len());
        let Some((runs, standing)) = (made.take_in)(&guest.inbox[..len], guest.read_limit) else {
            return NO_HANDLE;
        };

        guest.seat(runs, me, standing)
    })
}

/// Takes the actor `handle` names out of the instance: writes where it stands and its state, at [`Within`], as a child
/// taken out is written, and answers where they lie, as [`out`] does. Nothing, `0`, when it holds no such actor or
/// its state does not encode.
#[must_use]
pub fn take_out_at(types: &[Type], handle: u32) -> u64 {
    GUEST.with_borrow_mut(|guest| {
        let Some(slot) = guest.actors.get_mut(handle as usize) else {
            return 0;
        };
        let Slot::Live { runs, standing, .. } = mem::replace(slot, Slot::Empty) else {
            return 0;
        };
        guest.free.push(handle);
        let id = runs.declared().id();
        let made = types.iter().find(|made| made.declared.id() == id);
        guest.held.clear();
        let wrote = made.and_then(|made| (made.take_out)(&*runs, &standing, &mut guest.held));
        if wrote.is_none() {
            return 0;
        }

        (u64::from(address(guest.held.as_ptr())) << 32) | guest.held.len() as u64
    })
}

/// Runs the actor `handle` names on the `items` items written into the first `len` bytes of the inbox, each an
/// [`ItemHead`] and then its bytes, one turn for each until one fails or closes the actor. Answers where what the turns
/// asked for lies, as [`out`] does.
#[must_use]
pub fn turns(types: &[Type], calls: &'static Calls, handle: u32, items: u32, len: u32) -> u64 {
    GUEST.with_borrow_mut(|guest| {
        // What the last call asked for has been read by now, so its buffers are emptied for this batch, whose turns
        // all add to them.
        guest.open(calls);
        guest.out.clear();
        guest.out.resize(OutHead::BYTES, 0);
        let host = Host::new(calls);
        let Guest { actors, inbox, asked, out, instances, read_limit, .. } = &mut *guest;
        let read_limit = *read_limit;
        let Some(Slot::Live { runs, me, standing }) = actors.get_mut(handle as usize) else {
            return 0;
        };
        // Every actor a guest holds is an `Actor`, which computes each turn when it is asked for and has nothing to
        // prepare, so its batch goes straight to its turns.
        let inbox = &inbox[..(len as usize).min(inbox.len())];
        let mut rest = inbox;
        let mut ran: u32 = 0;
        let mut last = Last::Ran;
        let mut stood_before = 0;
        let mut counted = Counted::default();
        for _ in 0..items {
            let Some((read, ItemHead { source, chain, .. }, after)) = read_item(rest, read_limit) else {
                break;
            };
            rest = after;
            // What became of a request is handed to its handler with the request as it was filed, which the turn
            // reads.
            let filed;
            let item = match read {
                Inboxed::Item(item) => item,
                Inboxed::Answer { mail, asking } => {
                    filed = asking;
                    Item::Answer { mail, asking: &filed }
                }
                Inboxed::Unanswered { mail, asking } => {
                    filed = asking;
                    Item::Unanswered { mail, asking: &filed }
                }
            };
            TURN.set(ran);
            stood_before = asked.stood_count();
            let Standing { path, above } = &*standing;
            let keeps = Keeps::Claimed;
            // A guest's host answers for it what it owes, so the guest's own turn owes nothing (ADR-0017).
            let tie = Tie::TELL;
            let turning = Turning::new(*me, path, *above, &host, &host, asked, instances, keeps, 0, source, chain, tie);
            let result = runs.turn(item, turning);
            write_ends(out, asked, &mut counted);
            ran += 1;
            last = match result {
                Err(fault) => Last::Failed(fault),
                Ok(()) if counted.closes => Last::Closed,
                Ok(()) => Last::Ran,
            };
            // The engine asks for no turn after one that fails or closes the actor, so none is run.
            let ends_batch = !matches!(last, Last::Ran);
            if ends_batch {
                break;
            }
        }
        guest.write_records(types, ran, stood_before, last);

        guest.out_at()
    })
}

/// How the last turn a guest ran ended: as [`Stopped`] says, with the fault itself while the guest still holds it.
enum Last {
    Ran,
    Failed(Fault),
    Closed,
}

/// An item read from the inbox: one whole, or what became of a request with the request as its host filed it, which
/// the item refers to.
enum Inboxed<'a> {
    Item(Item<'a>),
    Answer { mail: Mail<'a>, asking: Asking },
    Unanswered { mail: Mail<'a>, asking: Asking },
}

/// One item read from the inbox, its head, and what follows it, its mail read within `read_limit` bytes.
fn read_item(items: &[u8], read_limit: usize) -> Option<(Inboxed<'_>, ItemHead, &[u8])> {
    let head = ItemHead::read(items)?;
    let rest = items.get(ItemHead::BYTES..)?;
    let (bytes, rest) = rest.split_at_checked(head.len as usize)?;
    let ItemHead { handed, source, chain, .. } = head;
    let mail = |kind, bytes| Mail::new(kind, bytes, source, chain.root, chain.depth, read_limit);
    let read = match handed {
        Handed::Mail { kind } => Inboxed::Item(Item::Mail(mail(kind, bytes))),
        Handed::Ended(gone) => Inboxed::Item(Item::Ended(ended(gone, source, bytes))),
        Handed::Answer { kind, request, context } => {
            let (bytes, asking) = filed(bytes, request, kind, context)?;
            Inboxed::Answer { mail: mail(kind, bytes), asking }
        }
        Handed::Unanswered { answer, request, context } => {
            let (bytes, asking) = filed(bytes, request, answer, context)?;
            Inboxed::Unanswered { mail: mail(KindId::of::<Unanswered>(), bytes), asking }
        }
    };

    Some((read, head, rest))
}

/// The bytes of what became of the request of the count `request`, answered with a kind `answer` names, and the
/// request as its host filed it: the last `context` of `bytes` are its context. Kept out of line, as most items are
/// mail.
#[cold]
#[inline(never)]
fn filed(bytes: &[u8], request: u64, answer: KindId, context: u32) -> Option<(&[u8], Asking)> {
    let (bytes, context) = bytes.split_at_checked(bytes.len().checked_sub(context as usize)?)?;

    Some((bytes, Asking { count: request, answer, context: context.to_vec() }))
}

/// The ending of `source`, which ended as `gone` says, a fault's bytes in `bytes`. Kept out of line: built in the
/// reading of mail, which most items are, it costs each piece of mail about a hundred instructions.
#[cold]
#[inline(never)]
fn ended(gone: Gone, source: Seated, bytes: &[u8]) -> Ended {
    let how = match gone {
        Gone::Closed => How::Closed,
        Gone::Failed { kind } => How::Failed(Fault::of_parts(kind, bytes.to_vec())),
        Gone::Earlier => How::Earlier,
    };

    Ended::new(source, how)
}

/// Where a child's state is, as its stood record is written: under a handle in this instance, taken out, at an
/// offset into the guest's held bytes and of a length, or with its host, as it is of other code.
enum Held {
    Here(u32),
    Out(usize, usize),
    Elsewhere,
}

impl Guest {
    /// Opens a call: what the last one asked for has been read by its host, so it is emptied, and the seats children
    /// are given are asked of the host through `calls`.
    fn open(&mut self, calls: &'static Calls) {
        self.asked.empty();
        let is_hosted = self.asked.seats_mut().is_of_host();
        if !is_hosted {
            *self.asked.seats_mut() = Seats::of_host(calls.take, calls.give_back);
        }
        let is_calling = self.instances.is_guest();
        if !is_calling {
            self.instances = Instances::of_guest(Arc::new(Calling { calls }), calls.made, self.read_limit);
        }
        let (_, puts, _) = self.asked.lent();
        let puts_hosted = puts.is_of_host();
        if !puts_hosted {
            *puts = Puts::of_host(calls.put);
        }
        self.held.clear();
        ELSEWHERE.with_borrow_mut(Vec::clear);
        TURN.set(0);
    }

    /// A guest that holds nothing yet, whose pin changes go to a buffer of its own from then on.
    fn new() -> Self {
        pins::take_own(&mut Vec::new());

        Self::default()
    }

    /// Keeps `runs`, the actor `me` names at `path`, under the handle of an empty slot, or a new one when none is.
    fn seat(&mut self, runs: Box<dyn Runs>, me: Seated, standing: Standing) -> u32 {
        let live = Slot::Live { runs, me, standing };
        let Some(handle) = self.free.pop() else {
            self.actors.push(live);
            return count(self.actors.len() - 1);
        };
        self.actors[handle as usize] = live;

        handle
    }

    /// Where what the last call asked for lies.
    fn out_at(&self) -> u64 {
        (u64::from(address(self.out.as_ptr())) << 32) | self.out.len() as u64
    }

    /// Keeps each child the call's turns stood up: under a handle, or taken out when its seat is in another part. The
    /// children stood up by the last turn, from the `stood_before`th, are dropped when it failed, as the engine drops
    /// them. Answers with each, and where it is held.
    fn keep_stood(&mut self, types: &[Type], stood_before: usize, is_failed: bool) -> Vec<(Seated, Seated, Held)> {
        let stood = self.asked.take_stood();
        let mut kept = Vec::with_capacity(stood.len());
        for (index, child) in stood.into_iter().enumerate() {
            let (me, path, above, runs, _) = child.into_parts();
            let Above::Actor(parent, _) = above else {
                continue;
            };
            let is_begun = (&*runs as &dyn Any).is::<Begun>();
            if is_begun {
                kept.push((me, parent, Held::Elsewhere));
                continue;
            }
            let standing = Standing { path, above };
            let is_dropped = is_failed && index >= stood_before;
            let is_elsewhere = ELSEWHERE.with_borrow(|elsewhere| elsewhere.contains(&me.seat));
            let held = if is_dropped {
                Held::Here(NO_HANDLE)
            } else if is_elsewhere {
                let at = self.held.len();
                let id = runs.declared().id();
                let made = types.iter().find(|made| made.declared.id() == id);
                let wrote = made.and_then(|made| (made.take_out)(&*runs, &standing, &mut self.held));
                match wrote {
                    Some(()) => Held::Out(at, self.held.len() - at),
                    None => Held::Here(NO_HANDLE),
                }
            } else {
                Held::Here(self.seat(runs, me, standing))
            };
            kept.push((me, parent, held));
        }

        kept
    }

    /// Writes the record of each send, line, spawn and child stood up in what the call's `ran` turns asked for, after
    /// their ends, and the head before them all, saying how the last of them ended. Each record says where its bytes
    /// lie in the guest's memory, which keeps them until the next call.
    fn write_records(&mut self, types: &[Type], ran: u32, stood_before: usize, last: Last) {
        let is_failed = matches!(last, Last::Failed(_));
        // Most calls spawn nothing, and a child that began was spawned, so they keep no child and write no key.
        let spawned =
            !self.asked.effects().is_empty() && self.asked.effects().iter().any(|effect| is_spawn(effect.what()));
        let mut kept = Vec::new();
        let mut keys = Vec::new();
        if spawned {
            kept = self.keep_stood(types, stood_before, is_failed);
            for effect in self.asked.effects() {
                let key = match effect.what() {
                    What::SpawnInstanced { key, .. } => to_bytes(key).unwrap_or_default(),
                    _ => Vec::new(),
                };
                keys.push((self.held.len(), key.len()));
                self.held.extend_from_slice(&key);
            }
        }

        let Self { out, asked, held, changes, .. } = self;
        let held_at = address(held.as_ptr());
        let in_held = |(at, len): (usize, usize)| Span { at: held_at + count(at), len: count(len) };
        let sends_at = out.len();
        let (refs, refers) = write_sends(out, asked, sends_at);
        let lines_at = out.len();
        for line in asked.lines() {
            let text = span(asked.text(line).as_bytes());
            out.extend_from_slice(&LineRecord { by: line.by, level: line.level, text }.to_bytes());
        }
        let spawns_at = out.len();
        for (effect, key) in asked.effects().iter().zip(keys) {
            let (from, key, seat) = match effect.what() {
                What::Spawn { from, seat } => (from, Keying::Singleton, *seat),
                What::SpawnInstanced { from, seat, .. } => (from, Keying::Keyed(in_held(key)), *seat),
                _ => continue,
            };
            let record = SpawnRecord { by: effect.seated(), kind: from.declared().id(), key, seat };
            out.extend_from_slice(&record.to_bytes());
        }
        let stood_at = out.len();
        for (me, above, held) in kept {
            let held = match held {
                Held::Here(handle) => Kept::Here { handle },
                Held::Out(at, len) => Kept::Out { state: in_held((at, len)) },
                Held::Elsewhere => Kept::Elsewhere,
            };
            out.extend_from_slice(&StoodRecord { me, above, held }.to_bytes());
        }
        let watches_at = out.len();
        let watches = asked.effects().iter().filter_map(|effect| match effect.what() {
            What::Watch { of, .. } => Some(WatchRecord { by: effect.seated(), of: of.seated() }),
            _ => None,
        });
        for watch in watches {
            out.extend_from_slice(&watch.to_bytes());
        }
        // Right after the last watch, where its host finds them (ADR-0015).
        for claim in asked.claims() {
            out.extend_from_slice(&ClaimRecord(*claim).to_bytes());
        }
        // Where its mail's references lie is written, and its host pins them itself, so the guest's own pins go now and
        // count among the call's changes.
        if refers {
            asked.let_go_of_pins();
        }
        let pins_at = out.len();
        let pins = write_pins(out, changes);
        let refs_at = out.len();
        for reference in &refs {
            out.extend_from_slice(&reference.to_bytes());
        }
        let fault_at = out.len();
        if let Last::Failed(fault) = &last {
            out.extend_from_slice(fault.bytes());
        }
        // Every address is taken once the buffer is written, as it moves when it grows.
        let base = address(out.as_ptr());
        let at = |offset: usize| base + count(offset);
        let stopped = match last {
            Last::Ran => Stopped::No,
            Last::Closed => Stopped::Closed,
            Last::Failed(fault) => Stopped::Failed {
                kind: fault.kind(),
                bytes: Span { at: at(fault_at), len: count(fault.bytes().len()) },
            },
        };
        let head = OutHead {
            ran,
            stopped,
            turns_at: at(OutHead::BYTES),
            sends_at: at(sends_at),
            lines_at: at(lines_at),
            spawns_at: at(spawns_at),
            stood_at: at(stood_at),
            watches_at: at(watches_at),
            pins_at: at(pins_at),
            pins,
            refs_at: at(refs_at),
            refs: count(refs.len()),
        };
        out[..OutHead::BYTES].copy_from_slice(&head.to_bytes());
    }
}

/// Whether `what` is the spawn of a child.
const fn is_spawn(what: &What) -> bool {
    matches!(what, What::Spawn { .. } | What::SpawnInstanced { .. })
}

/// Writes a [`SendRecord`] after `out` for each piece of mail in `asked`, from `sends_at`, and answers with where each
/// reference in them lies, and whether any piece holds a pin.
fn write_sends(out: &mut Vec<u8>, asked: &Asked, sends_at: usize) -> (Vec<RefRecord>, bool) {
    let mut refs = Vec::new();
    // Most calls send no reference to a blob, and their mail holds no pin.
    let refers = asked.arena().holds_pins();
    // Each request is filed as the mail that asks it is written, so the requests are in the order of that mail.
    let mut asks = asked.asks().iter();
    for mailed in asked.mailed() {
        let is_run = mailed.bytes.pieces() > 1;
        if is_run {
            write_run(out, asked, mailed, sends_at, &mut refs);
            continue;
        }
        let bytes = asked.arena().read(mailed.bytes).unwrap_or_default();
        if refers {
            note_refs(asked, mailed.bytes, sends_at, out.len(), &mut refs);
        }
        let request = requesting(mailed, &mut asks);
        let record = SendRecord { by: mailed.by, to: mailed.to, kind: mailed.kind, bytes: span(bytes), request };
        out.extend_from_slice(&record.to_bytes());
    }

    (refs, refers)
}

/// What the piece of mail `mailed` is to a request, as its host is told: the next of `asks` when it asks one.
fn requesting(mailed: &crate::Mailed, asks: &mut slice::Iter<'_, Asking>) -> Requesting {
    let Tied::Asks(_) = mailed.tie.read() else {
        return Requesting::Not;
    };
    let Some(asking) = asks.next() else {
        return Requesting::Not;
    };

    Requesting::Asks { answer: asking.answer, context: span(&asking.context) }
}

/// Writes a record for each piece of `mailed`, a run of mail in `asked`, after `out`, whose records begin at
/// `sends_at`: each is a send its host checks and hands over. Kept out of line, as a turn seldom sends a run, which is
/// of mail that asks nothing.
#[cold]
#[inline(never)]
fn write_run(out: &mut Vec<u8>, asked: &Asked, mailed: &crate::Mailed, sends_at: usize, refs: &mut Vec<RefRecord>) {
    for piece in mailed.bytes.each_piece() {
        let bytes = asked.arena().read(piece).unwrap_or_default();
        note_refs(asked, piece, sends_at, out.len(), refs);
        let request = Requesting::Not;
        let record = SendRecord { by: mailed.by, to: mailed.to, kind: mailed.kind, bytes: span(bytes), request };
        out.extend_from_slice(&record.to_bytes());
    }
}

/// Notes in `refs` where each reference in the piece of mail `piece` lies, which the send whose record is written at
/// `written`, of those written from `sends_at` in the out buffer, sends.
fn note_refs(asked: &Asked, piece: crate::Payload, sends_at: usize, written: usize, refs: &mut Vec<RefRecord>) {
    let pinned = asked.arena().pinned(piece);
    if matches!(pinned, Pinned::Nothing) {
        return;
    }
    let send = count((written - sends_at) / SendRecord::BYTES);
    pinned.each(|at, _| refs.push(RefRecord { send, at }));
}

/// Writes after `out` a [`PinRecord`] for each blob whose pins the guest's references to it have changed since this
/// was last done, with what the changes came to, and answers with how many: taken through `changes`, emptied and
/// kept for its room.
fn write_pins(out: &mut Vec<u8>, changes: &mut Vec<pins::Change>) -> u32 {
    pins::take_own(changes);
    if changes.is_empty() {
        return 0;
    }
    let mut summed: BTreeMap<(u32, u64), i64> = BTreeMap::new();
    for (blob, by) in changes.drain(..) {
        *summed.entry((blob.engine(), blob.bits())).or_default() += by;
    }
    let mut written = 0;
    for ((engine, bits), by) in summed {
        let is_changed = by != 0;
        if is_changed {
            out.extend_from_slice(&PinRecord { blob: BlobId::new(engine, bits), by }.to_bytes());
            written += 1;
        }
    }

    written
}

/// Writes the [`PinRecord`]s of what the call that just returned changed, for a call that writes no other records:
/// answers where they lie, as [`out`] does.
#[must_use]
pub fn pins_out() -> u64 {
    GUEST.with_borrow_mut(|guest| {
        let Guest { pinned, changes, .. } = &mut *guest;
        pinned.clear();
        write_pins(pinned, changes);

        (u64::from(address(guest.pinned.as_ptr())) << 32) | guest.pinned.len() as u64
    })
}

/// Runs the closing turn of the actor `handle` names, which runs as the number `why` says (ADR-0009): the actor ends
/// with it, and its state and the references it holds are dropped. Answers where what the turn asked for lies, as
/// [`out`] does: nothing, `0`, when it holds no such actor, or `why` names no reason.
#[must_use]
pub fn end_at(types: &[Type], calls: &'static Calls, handle: u32, why: u32) -> u64 {
    GUEST.with_borrow_mut(|guest| {
        let Some(why) = records::why_of(why) else {
            return 0;
        };
        let Some(slot) = guest.actors.get_mut(handle as usize) else {
            return 0;
        };
        let Slot::Live { runs, me, standing } = mem::replace(slot, Slot::Empty) else {
            return 0;
        };
        guest.free.push(handle);
        // What the last call asked for has been read by now, so its buffers are emptied for this one.
        guest.open(calls);
        guest.out.clear();
        guest.out.resize(OutHead::BYTES, 0);
        let host = Host::new(calls);
        let Guest { asked, instances, out, .. } = &mut *guest;
        let Standing { path, above } = &standing;
        let ended = runs.end(Closing::new(me, path, *above, &host, asked, instances, Keeps::Claimed, 0, why));
        write_ends(out, asked, &mut Counted::default());
        let last = match ended {
            Ok(()) => Last::Ran,
            Err(fault) => Last::Failed(fault),
        };
        guest.write_records(types, 1, 0, last);

        guest.out_at()
    })
}

/// Drops the actor `handle` names, with no closing turn, as the engine drops a child of a turn that failed: its state,
/// and the references it holds with it, which [`pins_out`] then tells its host of. Nothing when it holds no such actor.
pub fn drop_at(handle: u32) {
    GUEST.with_borrow_mut(|guest| {
        let Some(slot) = guest.actors.get_mut(handle as usize) else {
            return;
        };
        let was_live = matches!(mem::replace(slot, Slot::Empty), Slot::Live { .. });
        if was_live {
            guest.free.push(handle);
        }
    });
}

/// Sets the most bytes a read of one piece of mail, config, or state allocates in the guest to `bytes`, as its host's
/// reads do: the host sets it once, as it loads the instance, before any other call.
pub fn read_limit_at(bytes: u32) {
    GUEST.with_borrow_mut(|guest| guest.read_limit = bytes as usize);
}

/// Puts `bytes` as a blob in a call out to its host through `put`, the call out [`Calls::put`] wraps: answers with
/// its id.
///
/// # Errors
///
/// Why the host refused it.
pub fn put_with(put: extern "C" fn(u32, u32, u32) -> u64, bytes: &[u8]) -> Result<BlobId, Refused> {
    let mut id = [0u8; 12];
    let answer = put(address(bytes.as_ptr()), count(bytes.len()), address(id.as_mut_ptr()));
    records::refused(answer, |_| None)?;

    records::blob_at(&id).ok_or(Refused::NotMade)
}

/// How far a call's turns' ends have been counted in what they asked for, so each turn counts only what it added.
#[derive(Default)]
struct Counted {
    mailed: usize,
    effects: usize,
    ends: TurnEnds,
    /// Whether the turn counted last asked to close its actor.
    closes: bool,
}

/// Writes where the turn just run ends in `asked`'s buffers, after the ends of the turns before it, which `counted`
/// says, counting what it added: a send for each piece of mail, as a run's pieces are each a send.
fn write_ends(out: &mut Vec<u8>, asked: &Asked, counted: &mut Counted) {
    let Counted { mailed, effects, ends, closes } = counted;
    for added in &asked.mailed()[*mailed..] {
        ends.sends += added.bytes.pieces();
    }
    for effect in &asked.effects()[*effects..] {
        match effect.what() {
            What::Spawn { .. } | What::SpawnInstanced { .. } => ends.spawns += 1,
            What::Watch { .. } => ends.watches += 1,
            What::Close => *closes = true,
            _ => {}
        }
    }
    *mailed = asked.mailed().len();
    *effects = asked.effects().len();
    ends.lines = count(asked.lines().len());
    ends.stood = count(asked.stood_count());
    ends.claims = count(asked.claims().len());
    out.extend_from_slice(&ends.to_bytes());
}

/// Where `bytes` lie in the guest's memory.
fn span(bytes: &[u8]) -> Span {
    Span { at: address(bytes.as_ptr()), len: count(bytes.len()) }
}

/// A count of a guest's, which its 32-bit memory holds.
fn count(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// The number `bytes` hold, in ADR-0002's order: nothing when they are not eight.
fn u64_of(bytes: &[u8]) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.try_into().ok()?))
}

/// An address in the guest's memory, which is 32-bit.
fn address(pointer: *const u8) -> u32 {
    u32::try_from(pointer as usize).unwrap_or(u32::MAX)
}

/// The seat a host gave, `given` as its call out answers: its number, with the top bit set when it is in another part
/// than the actor whose call it is, which is kept.
#[must_use]
pub fn given_seat(given: u64) -> SeatNumber {
    let seat = SeatNumber::new(given & !ELSEWHERE_BIT);
    let is_elsewhere = given & ELSEWHERE_BIT != 0;
    if is_elsewhere {
        ELSEWHERE.with_borrow_mut(|elsewhere| elsewhere.push(seat));
    }

    seat
}

/// What a guest's turns reach other code through: its host, which begins a child of a native type or of another
/// blob's in a call out, and holds it (ADR-0015).
struct Calling {
    calls: &'static Calls,
}

impl Guests for Calling {
    /// Refused: a guest takes in no code.
    fn load(&self, _: BlobId, _: &[u8]) -> Result<(), Unfit> {
        Err(Unfit::NoRuntime)
    }

    fn unload(&self, _: BlobId) {}

    /// The type the guest holds `declared` declares, of the blob `code`: its host checks that the blob makes it as
    /// the guest holds it when it is asked to begin a child of it, and refuses the child otherwise.
    fn made(&self, code: BlobId, declared: &'static Declared) -> Option<&'static BlobType> {
        BLOB_TYPES.with_borrow_mut(|types| {
            let known = types.iter().copied().find(|made| made.is(code, declared));
            if let Some(made) = known {
                return Some(made);
            }
            let made: &'static BlobType = Box::leak(Box::new(BlobType { blob: code, declared }));
            types.push(made);

            Some(made)
        })
    }

    fn begin(&self, made: &'static BlobType, config: &[u8], birth: Birth<'_>) -> Result<Box<dyn Runs>, Refused> {
        let from = Other::Blob { engine: made.blob.engine(), bits: made.blob.bits() };

        self.spawn(from, made.declared, config, &birth)
    }

    fn begin_linked(
        &self,
        declared: &'static Declared,
        config: &[u8],
        birth: Birth<'_>,
    ) -> Result<Box<dyn Runs>, Refused> {
        self.spawn(Other::Linked, declared, config, &birth)
    }
}

impl Calling {
    /// Asks the host to begin `birth`'s child, of the type `declared` of the code `from`, from `config`: answers with
    /// what the guest holds of it, which it keeps nothing of, once it began.
    fn spawn(
        &self,
        from: Other,
        declared: &'static Declared,
        config: &[u8],
        birth: &Birth<'_>,
    ) -> Result<Box<dyn Runs>, Refused> {
        let by = match birth.above {
            Above::Actor(by, _) | Above::Elsewhere(by) => by,
            Above::Chassis => return Err(Refused::NotMade),
        };
        let spawning = Spawning {
            turn: TURN.get(),
            by: by.id.bits(),
            by_seat: by.seat.number(),
            me: birth.me.id.bits(),
            me_seat: birth.me.seat.number(),
            path: birth.path.clone(),
            from,
            kind: declared.name().clone(),
            declared: section::of(declared),
            config: config.to_vec(),
            refs: birth.refers.iter().map(|(at, _)| *at).collect(),
            carried: birth.carried.to_vec(),
        };
        let bytes = to_bytes(&spawning).map_err(|_| Refused::NotMade)?;
        let answer = (self.calls.spawn)(address(bytes.as_ptr()), count(bytes.len()));
        records::refused(answer, |len| {
            let mut fault = alloc::vec![0; 8 + len];
            (self.calls.refusal)(address(fault.as_mut_ptr()));
            let bytes = fault.split_off(8);
            let kind = from_bytes::<KindId>(&fault, &mut Budget::new(8)).ok()?;

            Some((kind, bytes))
        })?;

        Ok(Box::new(Begun { declared }))
    }
}

/// A child of other code that the guest's host began, as the guest's turn holds it: the guest keeps nothing of it,
/// and its host hands it to the engine (ADR-0015).
struct Begun {
    declared: &'static Declared,
}

impl Runs for Begun {
    fn declared(&self) -> &'static Declared {
        self.declared
    }

    /// Never: the guest never runs it.
    fn prepares(&self) -> bool {
        false
    }

    fn prepare(&mut self, _: Preparing<'_>) {}

    /// Nothing: the guest never runs it, as it keeps nothing of it.
    fn turn(&mut self, _: Item<'_>, _: Turning<'_>) -> Result<(), Fault> {
        Ok(())
    }

    fn end(self: Box<Self>, _: Closing<'_>) -> Result<(), Fault> {
        Ok(())
    }
}

/// What a guest's turn reads of the engine, asked of its host in calls out. It holds the declaration of no other
/// actor's type, so it checks no send, and leaves that to its host (ADR-0015).
pub struct Host {
    calls: &'static Calls,
    /// The blobs the call has read, each copied once into the guest's memory and kept for as long as the call.
    read: OnceCell<Box<Read>>,
}

/// A blob a call has read: its bytes, copied from its host, and the next blob read.
struct Read {
    blob: BlobId,
    bytes: Box<[u8]>,
    next: OnceCell<Box<Self>>,
}

impl Host {
    /// What a call's turns read of the engine, asked of its host through `calls`.
    const fn new(calls: &'static Calls) -> Self {
        Self { calls, read: OnceCell::new() }
    }

    /// The bytes of the blob `blob`, copied from its host the first time the call reads it: nothing when the host
    /// refuses, as the instance holds no pin on it and none was offered to the call.
    fn bytes(&self, blob: BlobId) -> Option<&[u8]> {
        let mut at = &self.read;
        while let Some(read) = at.get() {
            if read.blob == blob {
                return Some(&read.bytes);
            }
            at = &read.next;
        }
        let len = (self.calls.blob_len)(blob.engine(), blob.bits());
        let is_refused = len == u64::MAX;
        if is_refused {
            return None;
        }
        let mut bytes = alloc::vec![0; usize::try_from(len).ok()?].into_boxed_slice();
        (self.calls.blob_read)(blob.engine(), blob.bits(), address(bytes.as_mut_ptr()));
        let read = at.get_or_init(|| Box::new(Read { blob, bytes, next: OnceCell::new() }));

        Some(&read.bytes)
    }
}

impl Table for Host {
    /// The actor `path` names, asked of the host, which writes its id and seat into the guest's memory.
    fn resolve(&self, path: &ActorPath) -> Option<Seated> {
        let path = to_bytes(path).ok()?;
        let mut seated = [0u8; 16];
        let at = address(seated.as_mut_ptr());
        let is_live = (self.calls.resolve)(address(path.as_ptr()), count(path.len()), at) == 1;
        if !is_live {
            return None;
        }
        let (id, seat) = seated.split_at(8);

        Some(Seated { id: ActorId::new(u64_of(id)?), seat: SeatNumber::new(u64_of(seat)?) })
    }

    /// The child's id, asked of the host, which alone computes one.
    fn child(&self, parent: ActorId, segment: &PathSegment) -> ActorId {
        let naming = to_bytes(&Naming::of(segment)).unwrap_or_default();

        ActorId::new((self.calls.child)(parent.bits(), address(naming.as_ptr()), count(naming.len())))
    }

    fn is_live(&self, id: ActorId) -> bool {
        (self.calls.is_live)(id.bits()) == 1
    }

    /// Whether the type of the actor `id` stands for lists every one of `kinds`, asked of the host, which holds it.
    fn lists(&self, id: ActorId, kinds: &[KindId]) -> bool {
        let Ok(kinds) = to_bytes(&kinds.to_vec()) else {
            return false;
        };

        (self.calls.lists)(id.bits(), address(kinds.as_ptr()), count(kinds.len())) == 1
    }

    /// Nothing: the guest holds no other actor's type, and leaves every check of a send to its host, which fails the
    /// turn of a send its declaration does not reach where a native one stops the engine (ADR-0013, ADR-0015).
    fn declared(&self, _: ActorId) -> Option<&'static Declared> {
        None
    }

    fn held(&self, id: ActorId) -> bool {
        (self.calls.held)(id.bits()) == 1
    }

    fn child_seat(&self, id: ActorId) -> Option<SeatNumber> {
        let seat = (self.calls.child_seat)(id.bits());
        let is_none = seat == u64::MAX;

        if is_none {
            None
        } else {
            Some(SeatNumber::new(seat))
        }
    }

    /// The blob's bytes, copied into the guest's memory from its host's store.
    fn blob(&self, id: BlobId) -> Option<&[u8]> {
        self.bytes(id)
    }

    /// The blob's content hash, as its host keeps it or computes it.
    fn blob_hash(&self, id: BlobId) -> Option<BlobHash> {
        let mut hash = [0u8; 32];
        let is_hashed = (self.calls.blob_hash)(id.engine(), id.bits(), address(hash.as_mut_ptr())) == 1;

        is_hashed.then_some(BlobHash::from_bytes(hash))
    }
}

impl Children for Host {
    fn has_made(&self, child: ActorId) -> bool {
        (self.calls.has_made)(child.bits()) == 1
    }
}

/// Exports the calls a guest's host makes, for the actor types listed, imports the calls out it makes of its host,
/// and writes the section that declares them (ADR-0015). A guest's crate calls it once, at its root. Built for
/// anything but WebAssembly, it emits nothing, so a native program can name the guest's types and spawn them from
/// its blob.
#[macro_export]
macro_rules! guest {
    ($($actor:ty),+ $(,)?) => {
        #[cfg(target_family = "wasm")]
        const _: () = {
            const TYPES: &[&$crate::Declared] = &[$($crate::Declared::of::<$actor>()),+];
            const LEN: usize = $crate::section::len(TYPES);
            #[unsafe(link_section = "aethereal.declared")]
            #[used]
            static SECTION: [u8; LEN] = $crate::section::bytes::<LEN>(TYPES);
            static MADE: &[$crate::guest::Type] = &[$($crate::guest::Type::of::<$actor>()),+];

            #[link(wasm_import_module = "aethereal")]
            unsafe extern "C" {
                safe fn aethereal_child(parent: u64, naming: u32, len: u32) -> u64;
                safe fn aethereal_child_seat(id: u64) -> u64;
                safe fn aethereal_has_made(id: u64) -> u32;
                safe fn aethereal_resolve(path: u32, len: u32, seated: u32) -> u32;
                safe fn aethereal_is_live(id: u64) -> u32;
                safe fn aethereal_lists(id: u64, kinds: u32, len: u32) -> u32;
                safe fn aethereal_held(id: u64) -> u32;
                safe fn aethereal_take_seat() -> u64;
                safe fn aethereal_give_back_seat(seat: u64);
                safe fn aethereal_spawn(spawning: u32, len: u32) -> u64;
                safe fn aethereal_refusal(fault: u32);
                safe fn aethereal_blob_len(engine: u32, bits: u64) -> u64;
                safe fn aethereal_blob_read(engine: u32, bits: u64, at: u32);
                safe fn aethereal_blob_hash(engine: u32, bits: u64, at: u32) -> u32;
                safe fn aethereal_put(at: u32, len: u32, id: u32) -> u64;
            }

            fn put(bytes: Vec<u8>) -> Result<$crate::BlobId, $crate::Refused> {
                $crate::guest::put_with(aethereal_put, &bytes)
            }

            fn take() -> $crate::SeatNumber {
                $crate::guest::given_seat(aethereal_take_seat())
            }

            fn give_back(seat: $crate::SeatNumber) {
                aethereal_give_back_seat(seat.number());
            }

            static CALLS: $crate::guest::Calls = $crate::guest::Calls {
                child: aethereal_child,
                child_seat: aethereal_child_seat,
                has_made: aethereal_has_made,
                resolve: aethereal_resolve,
                is_live: aethereal_is_live,
                lists: aethereal_lists,
                held: aethereal_held,
                take,
                give_back,
                spawn: aethereal_spawn,
                refusal: aethereal_refusal,
                made: TYPES,
                blob_len: aethereal_blob_len,
                blob_read: aethereal_blob_read,
                blob_hash: aethereal_blob_hash,
                put,
            };

            #[unsafe(no_mangle)]
            extern "C" fn aethereal_read_limit(bytes: u32) {
                $crate::guest::read_limit_at(bytes)
            }

            #[unsafe(no_mangle)]
            extern "C" fn aethereal_pins() -> u64 {
                $crate::guest::pins_out()
            }

            #[unsafe(no_mangle)]
            extern "C" fn aethereal_end(handle: u32, why: u32) -> u64 {
                $crate::guest::end_at(MADE, &CALLS, handle, why)
            }

            #[unsafe(no_mangle)]
            extern "C" fn aethereal_drop(handle: u32) {
                $crate::guest::drop_at(handle)
            }

            #[unsafe(no_mangle)]
            extern "C" fn aethereal_inbox(len: u32) -> u32 {
                $crate::guest::inbox(len)
            }

            #[unsafe(no_mangle)]
            extern "C" fn aethereal_out() -> u64 {
                $crate::guest::out()
            }

            #[unsafe(no_mangle)]
            extern "C" fn aethereal_begin(of: u32, me: u64, seat: u64, len: u32) -> u32 {
                $crate::guest::begin_at(MADE, &CALLS, of, me, seat, len)
            }

            #[unsafe(no_mangle)]
            extern "C" fn aethereal_take_in(of: u32, me: u64, seat: u64, len: u32) -> u32 {
                $crate::guest::take_in_at(MADE, of, me, seat, len)
            }

            #[unsafe(no_mangle)]
            extern "C" fn aethereal_take_out(handle: u32) -> u64 {
                $crate::guest::take_out_at(MADE, handle)
            }

            #[unsafe(no_mangle)]
            extern "C" fn aethereal_turns(handle: u32, items: u32, len: u32) -> u64 {
                $crate::guest::turns(MADE, &CALLS, handle, items, len)
            }
        };
    };
}
