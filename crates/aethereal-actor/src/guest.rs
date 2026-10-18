//! What a guest's code runs on inside its instance: the calls its host makes, which [`guest!`](crate::guest) exports
//! (ADR-0015).
//!
//! A guest runs the same actors a native program does, on the same turns. Its
//! table answers nothing about other actors, so the turn checks no send: its
//! host checks each one as it reads the turn's mail. After each call the guest
//! writes what its turn asked for into one buffer of plain bytes, which its host
//! reads where it lies.

use crate::{Above, Actor, Actors, ActorsReader, Asked, BlobHash, BlobId, Chain, Children, Declared, Fault, Item};
use crate::{Batch, Beginning, Level, Mail, MailId, Peeked, Runs, SeatNumber, Seated, Table, Turning, What};
use aethereal_data::{ActorId, ActorPath, Budget, Decode, KindId, PathSegment, Reader, from_bytes};
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
        let inbox = mem::take(&mut guest.inbox);
        let len = (len as usize).min(inbox.len());
        let made = begin(&inbox[..len], me, &mut guest.asked);
        guest.inbox = inbox;
        let (handle, fault) = match made {
            Ok((runs, path)) => {
                let handle = u32::try_from(guest.actors.len()).unwrap_or(u32::MAX);
                guest.actors.push(Slot::Live { runs, me, path });
                (handle, None)
            }
            Err(fault) => (u32::MAX, Some(fault)),
        };
        guest.write_out(fault.as_ref());

        handle
    })
}

/// How many bytes each item in the inbox begins with: its kind's id, its source's id and seat, its chain's root,
/// sender and count, its depth, and the length of its bytes.
pub const ITEM_HEAD: usize = 8 + 8 + 8 + 8 + 8 + 4 + 4;

/// Runs the actor `handle` names on the `items` items written into the first `len` bytes of the inbox, each a head of
/// [`ITEM_HEAD`] bytes and then its bytes, one turn for each until one fails. Answers where what the turns asked for
/// lies, as [`out`] does: how many turns ran, then for each whether it failed, and with what, its mail, and its lines.
#[must_use]
pub fn turns(handle: u32, items: u32, len: u32) -> u64 {
    GUEST.with_borrow_mut(|guest| {
        let Guest { actors, inbox, asked, out } = &mut *guest;
        out.clear();
        out.extend_from_slice(&0_u32.to_le_bytes());
        let Some(Slot::Live { runs, me, path }) = actors.get_mut(handle as usize) else {
            return 0;
        };
        let inbox = &inbox[..(len as usize).min(inbox.len())];
        runs.prepare(&Inboxed { items: inbox, fits: items as usize });
        let mut rest = inbox;
        let mut ran: u32 = 0;
        for _ in 0..items {
            let Some((Peeked { item, source, chain }, after)) = read_item(rest) else {
                break;
            };
            rest = after;
            let turning = Turning::new(*me, path, Above::Chassis, &Host, &(), asked, 0, source, chain);
            let result = runs.turn(item, turning);
            let failed = result.is_err();
            let closes = asks_close(asked);
            write_turn(out, asked, result.err().as_ref());
            ran += 1;
            // The engine asks for no turn after one that fails or closes the actor, so none is run.
            let ends = failed || closes;
            if ends {
                break;
            }
        }
        out[..4].copy_from_slice(&ran.to_le_bytes());

        guest.out_at()
    })
}

/// The batch a guest's host wrote into its inbox.
struct Inboxed<'a> {
    items: &'a [u8],
    fits: usize,
}

/// One item read from the inbox, and what follows it.
fn read_item(items: &[u8]) -> Option<(Peeked<'_>, &[u8])> {
    let (head, rest) = items.split_at_checked(ITEM_HEAD)?;
    let mut budget = Budget::new(8);
    let kind = from_bytes::<KindId>(&head[..8], &mut budget).ok()?;
    let number = |at: usize| u64::from_le_bytes(head[at..at + 8].try_into().unwrap_or_default());
    let source = Seated { id: ActorId::new(number(8)), seat: SeatNumber::new(number(16)) };
    let root = MailId { sender: ActorId::new(number(24)), count: number(32) };
    let depth = u32::from_le_bytes(head[40..44].try_into().unwrap_or_default());
    let len = u32::from_le_bytes(head[44..48].try_into().unwrap_or_default()) as usize;
    let (bytes, rest) = rest.split_at_checked(len)?;
    let mail = Mail::new(kind, bytes, source, root, depth, READ_LIMIT);

    Some((Peeked { item: Item::Mail(mail), source, chain: Chain { root, depth } }, rest))
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

    /// Writes what the last call, a `begin`, asked for into `out`, as one turn, as [`turns`] writes its turns.
    fn write_out(&mut self, fault: Option<&Fault>) {
        let Self { out, asked, .. } = self;
        out.clear();
        out.extend_from_slice(&1_u32.to_le_bytes());
        write_turn(out, asked, fault);
    }
}

/// Whether the turn that filled `asked` asked to close its actor.
fn asks_close(asked: &Asked) -> bool {
    asked.effects().iter().any(|effect| matches!(effect.what(), What::Close))
}

/// Writes what one turn asked for after what `out` holds, and empties the buffers it was asked into: whether it
/// failed, and with what fault; the mail it sent; and the lines it logged.
fn write_turn(out: &mut Vec<u8>, asked: &mut Asked, fault: Option<&Fault>) {
    match fault {
        None => out.extend_from_slice(&0_u32.to_le_bytes()),
        Some(fault) => {
            out.extend_from_slice(&1_u32.to_le_bytes());
            out.extend_from_slice(&fault.kind().plain_bytes());
            write_bytes(out, fault.bytes());
        }
    }
    out.extend_from_slice(&u32::from(asks_close(asked)).to_le_bytes());
    let mailed = asked.mailed();
    out.extend_from_slice(&count(mailed.len()).to_le_bytes());
    for piece in mailed {
        out.extend_from_slice(&piece.to.id.bits().to_le_bytes());
        out.extend_from_slice(&piece.to.seat.number().to_le_bytes());
        out.extend_from_slice(&piece.kind.plain_bytes());
        write_bytes(out, asked.arena().read(piece.bytes).unwrap_or_default());
    }
    let lines = asked.lines();
    out.extend_from_slice(&count(lines.len()).to_le_bytes());
    for line in lines {
        let level: u32 = match line.level {
            Level::Debug => 0,
            Level::Info => 1,
            Level::Warn => 2,
            Level::Error => 3,
        };
        out.extend_from_slice(&level.to_le_bytes());
        write_bytes(out, asked.text(line).as_bytes());
    }

    asked.empty();
}

/// Writes `bytes` after their count.
fn write_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&count(bytes.len()).to_le_bytes());
    out.extend_from_slice(bytes);
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
