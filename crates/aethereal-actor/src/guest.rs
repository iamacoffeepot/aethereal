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
//! host to take into that part's instance.

use crate::records::{
    Born, Gone, Handed, ItemHead, Kept, Keying, LineRecord, Naming, OutHead, Parent, SendRecord, Span, SpawnRecord,
    StoodRecord, Stopped, TurnEnds, WatchRecord,
};
use crate::{Above, Actor, Actors, ActorsReader, Asked, BlobHash, BlobId, Children, Declared, Ended, Fault, How, Item};
use crate::{Beginning, Instances, Mail, Peeked, Runs, SeatNumber, Seated, Seats};
use crate::{Table, Turning, What, Within, WithinReader, WithinWriter};
use aethereal_data::{
    ActorId, ActorPath, Budget, Decode, DecodeError, Encode, PathSegment, Reader, Writer, from_bytes, to_bytes,
};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::any::Any;
use core::cell::RefCell;
use core::mem;

/// Begins an actor of one type as it is seated, where it stands, from its config's bytes.
type Begin = fn(&[u8], Seated, &Standing, &Host, &mut Asked, &mut Instances) -> Result<Box<dyn Runs>, Fault>;

/// Takes an actor of one type in from where it stands and its state, as [`take_out`] wrote them.
type TakeIn = fn(&[u8]) -> Option<(Box<dyn Runs>, Standing)>;

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
    /// Whether the table held the actor of the id as the step began, or it was stood up since in the part: one when
    /// it was.
    pub held: extern "C" fn(u64) -> u32,
    /// A seat for a child, which [`given_seat`] reads from what the host answers.
    pub take: fn() -> SeatNumber,
    /// Hands back the seat of a child that never began.
    pub give_back: fn(SeatNumber),
}

/// What the guest holds: its actors, where its host writes what it hands in, and where it writes what it asked for.
#[derive(Default)]
struct Guest {
    actors: Vec<Slot>,
    inbox: Vec<u8>,
    out: Vec<u8>,
    /// The bytes the records point to that no buffer of `asked` holds: the keys of children spawned, and the paths and
    /// state of children taken out.
    held: Vec<u8>,
    asked: Asked,
    /// What its actors' turns are lent: a guest's actors are its own code, run in no instance, so it is always empty.
    instances: Instances,
}

/// One actor of the guest, by the handle its host knows it by.
enum Slot {
    Live {
        runs: Box<dyn Runs>,
        me: Seated,
        standing: Standing,
    },
    /// Taken out of the instance, for its host to keep or take into another: its handle is not used again.
    Out,
}

std::thread_local! {
    static GUEST: RefCell<Guest> = RefCell::new(Guest::default());
    /// The seats the host gave in the call under way that are in another part than the actor whose call it is.
    static ELSEWHERE: RefCell<Vec<SeatNumber>> = const { RefCell::new(Vec::new()) };
}

/// The most bytes a guest's read of its mail allocates.
const READ_LIMIT: usize = 1 << 20;

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
    let mut budget = Budget::new(READ_LIMIT);
    let mut reader = Reader::<Actors>::new(config, &mut budget, ActorsReader::new());
    let config = A::Config::decode(&mut reader)?;
    let Standing { path, above } = standing;
    let actor = A::begin(config, &mut Beginning::beneath::<A>(me, path, *above, host, asked, instances))?;

    Ok(Box::new(actor))
}

/// An actor of type `A` taken in from `bytes`, its path and state as [`take_out`] wrote them: nothing when they do not
/// read back.
fn take_in<A: Actor>(bytes: &[u8]) -> Option<(Box<dyn Runs>, Standing)> {
    let mut budget = Budget::new(READ_LIMIT);
    let mut reader = Reader::<Within>::new(bytes, &mut budget, WithinReader::new());
    let born = Born::decode(&mut reader).ok()?;
    let actor = A::decode(&mut reader).ok()?;

    Some((Box::new(actor), Standing::of(born)))
}

/// Writes `runs`, an actor of type `A` that stands where `standing` says, after `out`: where it stands and then its
/// state, at [`Within`]. Nothing when it is not of type `A`, or does not encode.
fn take_out<A: Actor>(runs: &dyn Runs, standing: &Standing, out: &mut Vec<u8>) -> Option<()> {
    let actor = (runs as &dyn Any).downcast_ref::<A>()?;
    let mut writer = Writer::<Within>::new(WithinWriter::new());
    standing.born().encode(&mut writer).ok()?;
    actor.encode(&mut writer).ok()?;
    out.extend_from_slice(&writer.into_bytes());

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
        let began = standing(&inbox[..len]).map_err(Fault::from).and_then(|(standing, config)| {
            let Guest { asked, instances, .. } = &mut *guest;
            let runs = (made.begin)(config, me, &standing, &Host { calls }, asked, instances)?;
            Ok((runs, standing))
        });
        guest.inbox = inbox;
        let (handle, last) = match began {
            Ok((runs, standing)) => (guest.seat(runs, me, standing), Last::Ran),
            Err(fault) => (NO_HANDLE, Last::Failed(fault)),
        };
        guest.out.clear();
        guest.out.resize(OutHead::BYTES, 0);
        write_ends(&mut guest.out, &guest.asked);
        guest.write_records(types, 1, 0, last);

        handle
    })
}

/// Where an actor to begin stands, and its config, from what its host handed in: a [`Born`], its length first, and then
/// the config's bytes.
fn standing(handed: &[u8]) -> Result<(Standing, &[u8]), DecodeError> {
    let (len, rest) = handed.split_at_checked(4).ok_or(DecodeError::UnexpectedEnd)?;
    let len = len.try_into().map_or(0, u32::from_le_bytes) as usize;
    let (born, config) = rest.split_at_checked(len).ok_or(DecodeError::UnexpectedEnd)?;
    let born = from_bytes::<Born>(born, &mut Budget::new(READ_LIMIT))?;

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
        let Some((runs, standing)) = (made.take_in)(&guest.inbox[..len]) else {
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
        let Slot::Live { runs, standing, .. } = mem::replace(slot, Slot::Out) else {
            return 0;
        };
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
        let host = Host { calls };
        let Guest { actors, inbox, asked, out, instances, .. } = &mut *guest;
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
        for _ in 0..items {
            let Some((Peeked { item, source, chain }, after)) = read_item(rest) else {
                break;
            };
            rest = after;
            stood_before = asked.stood_count();
            let Standing { path, above } = &*standing;
            let turning = Turning::new(*me, path, *above, &host, &host, asked, instances, 0, source, chain);
            let result = runs.turn(item, turning);
            write_ends(out, asked);
            ran += 1;
            last = match result {
                Err(fault) => Last::Failed(fault),
                Ok(()) if asks_close(asked) => Last::Closed,
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

/// One item read from the inbox, and what follows it.
fn read_item(items: &[u8]) -> Option<(Peeked<'_>, &[u8])> {
    let head = ItemHead::read(items)?;
    let rest = items.get(ItemHead::BYTES..)?;
    let (bytes, rest) = rest.split_at_checked(head.len as usize)?;
    let ItemHead { handed, source, chain, .. } = head;
    let item = match handed {
        Handed::Mail { kind } => Item::Mail(Mail::new(kind, bytes, source, chain.root, chain.depth, READ_LIMIT)),
        Handed::Ended(gone) => Item::Ended(ended(gone, source, bytes)),
    };

    Some((Peeked { item, source, chain }, rest))
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

/// Where a child's state is, as its stood record is written: under a handle in this instance, or taken out, at an
/// offset into the guest's held bytes and of a length.
enum Held {
    Here(u32),
    Out(usize, usize),
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
        self.held.clear();
        ELSEWHERE.with_borrow_mut(Vec::clear);
    }

    /// Keeps `runs`, the actor `me` names at `path`, under a new handle.
    fn seat(&mut self, runs: Box<dyn Runs>, me: Seated, standing: Standing) -> u32 {
        let handle = count(self.actors.len());
        self.actors.push(Slot::Live { runs, me, standing });

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
            let (me, path, above, runs) = child.into_parts();
            let Above::Actor(parent, _) = above else {
                continue;
            };
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

        let Self { out, asked, held, .. } = self;
        let held_at = address(held.as_ptr());
        let in_held = |(at, len): (usize, usize)| Span { at: held_at + count(at), len: count(len) };
        let sends_at = out.len();
        for piece in asked.mailed() {
            let bytes = asked.arena().read(piece.bytes).unwrap_or_default();
            let record = SendRecord { by: piece.by, to: piece.to, kind: piece.kind, bytes: span(bytes) };
            out.extend_from_slice(&record.to_bytes());
        }
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
        };
        out[..OutHead::BYTES].copy_from_slice(&head.to_bytes());
    }
}

/// Whether the turn that filled `asked` asked to close its actor.
fn asks_close(asked: &Asked) -> bool {
    asked.effects().iter().any(|effect| matches!(effect.what(), What::Close))
}

/// Whether `what` is the spawn of a child.
const fn is_spawn(what: &What) -> bool {
    matches!(what, What::Spawn { .. } | What::SpawnInstanced { .. })
}

/// Writes where the turn just run ends in `asked`'s buffers, after the ends of the turns before it.
fn write_ends(out: &mut Vec<u8>, asked: &Asked) {
    let mut spawns = 0;
    let mut watches = 0;
    for effect in asked.effects() {
        match effect.what() {
            What::Spawn { .. } | What::SpawnInstanced { .. } => spawns += 1,
            What::Watch { .. } => watches += 1,
            _ => {}
        }
    }
    let ends = TurnEnds {
        sends: count(asked.mailed().len()),
        lines: count(asked.lines().len()),
        spawns,
        stood: count(asked.stood_count()),
        watches,
    };
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

/// What a guest's turn reads of the engine, asked of its host in calls out. It holds the declaration of no other
/// actor's type, so it checks no send, and leaves that to its host (ADR-0015).
pub struct Host {
    calls: &'static Calls,
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

    /// Nothing, for now: a blob's bytes lie in the host's store, and how a guest reads them, and what its records
    /// hold of the blobs it pins, is not decided (ADR-0015).
    fn blob(&self, _: BlobId) -> Option<&[u8]> {
        None
    }

    /// Nothing, for now, as for [`Self::blob`].
    fn blob_hash(&self, _: BlobId) -> Option<BlobHash> {
        None
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
                safe fn aethereal_held(id: u64) -> u32;
                safe fn aethereal_take_seat() -> u64;
                safe fn aethereal_give_back_seat(seat: u64);
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
                held: aethereal_held,
                take,
                give_back,
            };

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
