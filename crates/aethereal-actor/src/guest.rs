//! What a guest's code runs on inside its instance: the calls its host makes, which [`guest!`](crate::guest) exports
//! (ADR-0015).
//!
//! A guest runs the same actors a native program does, on the same turns. Its
//! table answers nothing about other actors, so the turn checks no send: its
//! host checks each one as it reads the turn's mail. After each call the guest
//! writes a record of what each of its turns asked for, in the layout of
//! [`records`](crate::records), and its host reads each where it lies, the
//! bytes of mail and the text of lines among them.

use crate::records::{ItemHead, LineRecord, OutHead, SendRecord, Span, Stopped, TurnEnds};
use crate::{Above, Actor, Actors, ActorsReader, Asked, BlobHash, BlobId, Children, Declared, Fault, Item};
use crate::{Batch, Beginning, Mail, Peeked, Runs, SeatNumber, Seated, Table, Turning, What};
use aethereal_data::{ActorId, ActorPath, Budget, Decode, PathSegment, Reader};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::mem;

/// Begins an actor of one type from its config's bytes.
pub type Begin = fn(&[u8], Seated, &mut Asked) -> Result<(Box<dyn Runs>, ActorPath), Fault>;

/// What the guest holds: its actors, where its host writes what it hands in, and where it writes what it asked for.
#[derive(Default)]
struct Guest {
    actors: Vec<Slot>,
    inbox: Vec<u8>,
    out: Vec<u8>,
    asked: Asked,
}

/// One actor of the guest, by the handle its host knows it by.
enum Slot {
    Live { runs: Box<dyn Runs>, me: Seated, path: ActorPath },
}

std::thread_local! {
    static GUEST: RefCell<Guest> = RefCell::new(Guest::default());
}

/// The most bytes a guest's read of its mail allocates.
const READ_LIMIT: usize = 1 << 20;

/// Begins an actor of type `A`: a root, as its host makes it.
///
/// # Errors
///
/// The fault its `begin` returned, or one for a config that did not decode.
pub fn begin<A: Actor>(config: &[u8], me: Seated, asked: &mut Asked) -> Result<(Box<dyn Runs>, ActorPath), Fault> {
    let mut budget = Budget::new(READ_LIMIT);
    let mut reader = Reader::<Actors>::new(config, &mut budget, ActorsReader::new());
    let config = A::Config::decode(&mut reader)?;
    let path = ActorPath::from_root(&PathSegment::new(A::NAME, &()));
    let actor = A::begin(config, &mut Beginning::new::<A>(me, &path, &Host, asked))?;

    Ok((Box::new(actor), path))
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

/// Begins an actor of the `of`th type in `begins`, as `me` and `seat` name it, from the config in the inbox: its
/// handle, or `u32::MAX` when its `begin` failed.
#[must_use]
pub fn begin_at(begins: &[Begin], of: u32, me: u64, seat: u64, len: u32) -> u32 {
    GUEST.with_borrow_mut(|guest| {
        let me = Seated { id: ActorId::new(me), seat: SeatNumber::new(seat) };
        let Some(begin) = begins.get(of as usize) else {
            return u32::MAX;
        };
        // What the last call asked for has been read by now, so its buffers are emptied for this one.
        guest.asked.empty();
        let inbox = mem::take(&mut guest.inbox);
        let len = (len as usize).min(inbox.len());
        let made = begin(&inbox[..len], me, &mut guest.asked);
        guest.inbox = inbox;
        let (handle, last) = match made {
            Ok((runs, path)) => {
                let handle = u32::try_from(guest.actors.len()).unwrap_or(u32::MAX);
                guest.actors.push(Slot::Live { runs, me, path });
                (handle, Last::Ran)
            }
            Err(fault) => (u32::MAX, Last::Failed(fault)),
        };
        let Guest { asked, out, .. } = guest;
        out.clear();
        out.resize(OutHead::BYTES, 0);
        write_ends(out, asked);
        write_records(out, asked, 1, last);

        handle
    })
}

/// Runs the actor `handle` names on the `items` items written into the first `len` bytes of the inbox, each an
/// [`ItemHead`] and then its bytes, one turn for each until one fails or closes the actor. Answers where what the turns
/// asked for lies, as [`out`] does.
#[must_use]
pub fn turns(handle: u32, items: u32, len: u32) -> u64 {
    GUEST.with_borrow_mut(|guest| {
        let Guest { actors, inbox, asked, out } = &mut *guest;
        // What the last call asked for has been read by now, so its buffers are emptied for this batch, whose turns
        // all add to them.
        asked.empty();
        out.clear();
        out.resize(OutHead::BYTES, 0);
        let Some(Slot::Live { runs, me, path }) = actors.get_mut(handle as usize) else {
            return 0;
        };
        let inbox = &inbox[..(len as usize).min(inbox.len())];
        runs.prepare(&Inboxed { items: inbox, fits: items as usize });
        let mut rest = inbox;
        let mut ran: u32 = 0;
        let mut last = Last::Ran;
        for _ in 0..items {
            let Some((Peeked { item, source, chain }, after)) = read_item(rest) else {
                break;
            };
            rest = after;
            let turning = Turning::new(*me, path, Above::Chassis, &Host, &(), asked, 0, source, chain);
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
        write_records(out, asked, ran, last);

        guest.out_at()
    })
}

/// How the last turn a guest ran ended: as [`Stopped`] says, with the fault itself while the guest still holds it.
enum Last {
    Ran,
    Failed(Fault),
    Closed,
}

/// The batch a guest's host wrote into its inbox.
struct Inboxed<'a> {
    items: &'a [u8],
    fits: usize,
}

/// One item read from the inbox, and what follows it.
fn read_item(items: &[u8]) -> Option<(Peeked<'_>, &[u8])> {
    let head = ItemHead::read(items)?;
    let rest = items.get(ItemHead::BYTES..)?;
    let (bytes, rest) = rest.split_at_checked(head.len as usize)?;
    let ItemHead { kind, source, chain, .. } = head;
    let mail = Mail::new(kind, bytes, source, chain.root, chain.depth, READ_LIMIT);

    Some((Peeked { item: Item::Mail(mail), source, chain }, rest))
}

impl Batch for Inboxed<'_> {
    fn each(&self, visit: &mut dyn FnMut(Peeked<'_>)) {
        let mut items = self.items;
        for _ in 0..self.fits {
            let Some((peeked, rest)) = read_item(items) else {
                return;
            };
            visit(peeked);
            items = rest;
        }
    }
}

impl Guest {
    /// Where what the last call asked for lies.
    fn out_at(&self) -> u64 {
        (u64::from(address(self.out.as_ptr())) << 32) | self.out.len() as u64
    }
}

/// Whether the turn that filled `asked` asked to close its actor.
fn asks_close(asked: &Asked) -> bool {
    asked.effects().iter().any(|effect| matches!(effect.what(), What::Close))
}

/// Writes where the turn just run ends in `asked`'s buffers, after the ends of the turns before it.
fn write_ends(out: &mut Vec<u8>, asked: &Asked) {
    let ends = TurnEnds { sends: count(asked.mailed().len()), lines: count(asked.lines().len()) };
    out.extend_from_slice(&ends.to_bytes());
}

/// Writes the record of each send and line in `asked`, after the ends of the `ran` turns that asked for them, and the
/// head before them all, saying how the last of them ended. Each record says where its bytes lie in `asked`'s arena,
/// which keeps them until the next call; a fault's bytes are copied after the records.
fn write_records(out: &mut Vec<u8>, asked: &Asked, ran: u32, last: Last) {
    let sends_at = out.len();
    for piece in asked.mailed() {
        let bytes = asked.arena().read(piece.bytes).unwrap_or_default();
        out.extend_from_slice(&SendRecord { to: piece.to, kind: piece.kind, bytes: span(bytes) }.to_bytes());
    }
    let lines_at = out.len();
    for line in asked.lines() {
        out.extend_from_slice(&LineRecord { level: line.level, text: span(asked.text(line).as_bytes()) }.to_bytes());
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
        Last::Failed(fault) => {
            Stopped::Failed { kind: fault.kind(), bytes: Span { at: at(fault_at), len: count(fault.bytes().len()) } }
        }
    };
    let head = OutHead { ran, stopped, turns_at: at(OutHead::BYTES), sends_at: at(sends_at), lines_at: at(lines_at) };
    out[..OutHead::BYTES].copy_from_slice(&head.to_bytes());
}

/// Where `bytes` lie in the guest's memory.
fn span(bytes: &[u8]) -> Span {
    Span { at: address(bytes.as_ptr()), len: count(bytes.len()) }
}

/// A count of a guest's, which its 32-bit memory holds.
fn count(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// An address in the guest's memory, which is 32-bit.
fn address(pointer: *const u8) -> u32 {
    u32::try_from(pointer as usize).unwrap_or(u32::MAX)
}

/// What a guest's turn reads of the engine: nothing yet, so it checks no send, and leaves that to its host.
struct Host;

impl Table for Host {
    fn resolve(&self, _: &ActorPath) -> Option<Seated> {
        None
    }

    /// A guest's turn reads no child yet: only the engine computes an id, and reading one is a call out to its host.
    fn child(&self, _: ActorId, _: &PathSegment) -> ActorId {
        panic!("a guest reads no child yet");
    }

    fn is_live(&self, _: ActorId) -> bool {
        true
    }

    fn declared(&self, _: ActorId) -> Option<&'static Declared> {
        None
    }

    fn held(&self, _: ActorId) -> bool {
        true
    }

    fn child_seat(&self, _: ActorId) -> Option<SeatNumber> {
        None
    }

    fn blob(&self, _: BlobId) -> Option<&[u8]> {
        None
    }

    fn blob_hash(&self, _: BlobId) -> Option<BlobHash> {
        None
    }
}

impl Children for Host {
    fn has_made(&self, _: ActorId) -> bool {
        false
    }
}

/// Exports the calls a guest's host makes, for the actor types listed, and writes the section that declares them
/// (ADR-0015). A guest's crate calls it once, at its root.
#[macro_export]
macro_rules! guest {
    ($($actor:ty),+ $(,)?) => {
        const _: () = {
            const TYPES: &[&$crate::Declared] = &[$($crate::Declared::of::<$actor>()),+];
            const LEN: usize = $crate::section::len(TYPES);
            #[unsafe(link_section = "aethereal.declared")]
            #[used]
            static SECTION: [u8; LEN] = $crate::section::bytes::<LEN>(TYPES);
            static BEGINS: &[$crate::guest::Begin] = &[$($crate::guest::begin::<$actor>),+];

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
                $crate::guest::begin_at(BEGINS, of, me, seat, len)
            }

            #[unsafe(no_mangle)]
            extern "C" fn aethereal_turns(handle: u32, items: u32, len: u32) -> u64 {
                $crate::guest::turns(handle, items, len)
            }
        };
    };
}
