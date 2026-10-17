//! What a guest's code runs on inside its instance: the calls its host makes, which [`guest!`](crate::guest) exports
//! (ADR-0015).
//!
//! A guest runs the same actors a native program does, on the same turns. Its
//! table answers nothing about other actors, so the turn checks no send: its
//! host checks each one as it reads the turn's mail. After each call the guest
//! writes what its turn asked for into one buffer of plain bytes, which its host
//! reads where it lies.

use crate::{Above, Actor, Actors, ActorsReader, Asked, BlobHash, BlobId, Chain, Children, Declared, Fault, Item};
use crate::{Beginning, Level, Mail, MailId, Runs, SeatNumber, Seated, Table, Turning};
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
        let began = begin(&inbox[..len.min(inbox.len() as u32) as usize], me, &mut guest.asked);
        guest.inbox = inbox;
        let (handle, fault) = match began {
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

/// Hands the actor `handle` names the `len` bytes of mail in the inbox: its kind's id, then its bytes, from the actor
/// `source` and `source_seat` name, at `depth` in the chain from the mail `root` sent as its `count`th. Answers where
/// what the turn asked for lies, as [`out`] does: it says whether the turn failed.
#[must_use]
pub fn turn(handle: u32, len: u32, source: u64, source_seat: u64, root: u64, count: u64, depth: u32) -> u64 {
    GUEST.with_borrow_mut(|guest| {
        let Guest { actors, inbox, asked, .. } = &mut *guest;
        let Some(Slot::Live { runs, me, path }) = actors.get_mut(handle as usize) else {
            return 0;
        };
        let mut budget = Budget::new(8);
        let inbox = &inbox[..(len as usize).min(inbox.len())];
        let (kind_bytes, bytes) = inbox.split_at(inbox.len().min(8));
        let Ok(kind) = from_bytes::<KindId>(kind_bytes, &mut budget) else {
            return 0;
        };
        let source = Seated { id: ActorId::new(source), seat: SeatNumber::new(source_seat) };
        let chain = Chain { root: MailId { sender: ActorId::new(root), count }, depth };
        let mail = Mail::new(kind, bytes, source, chain.root, depth, READ_LIMIT);
        let turning = Turning::new(*me, path, Above::Chassis, &Host, &(), asked, 0, source, chain);
        let handled = runs.handle(Item::Mail(mail), turning);
        let fault = handled.err();
        guest.write_out(fault.as_ref());

        guest.out_at()
    })
}

impl Guest {
    /// Where what the last call asked for lies.
    fn out_at(&self) -> u64 {
        (u64::from(address(self.out.as_ptr())) << 32) | self.out.len() as u64
    }

    /// Writes what the last call asked for into `out`, and empties the buffers it was asked into: whether it failed,
    /// and with what fault; the mail it sent; and the lines it logged.
    fn write_out(&mut self, fault: Option<&Fault>) {
        let Self { out, asked, .. } = self;
        out.clear();
        match fault {
            None => out.extend_from_slice(&0_u32.to_le_bytes()),
            Some(fault) => {
                out.extend_from_slice(&1_u32.to_le_bytes());
                out.extend_from_slice(&fault.kind().plain_bytes());
                write_bytes(out, fault.bytes());
            }
        }
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
            extern "C" fn aethereal_turn(handle: u32, len: u32, source: u64, source_seat: u64, root: u64, count: u64, depth: u32) -> u64 {
                $crate::guest::turn(handle, len, source, source_seat, root, count, depth)
            }
        };
    };
}
