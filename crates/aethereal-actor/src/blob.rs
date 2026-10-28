//! A blob: a buffer of bytes the engine holds and nobody changes, and the references that pin it (ADR-0014).

use crate::Refused;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::mem;

/// Which blob a reference pins: the engine that holds it, and a number that engine gave it when it was put and gives
/// no other (ADR-0014).
///
/// It names a blob in its own engine only, and a process may hold several engines. A blob that crosses to another
/// engine is named there by its content's hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlobId {
    engine: u32,
    bits: u64,
}

impl BlobId {
    /// The id of the blob numbered `bits` in the engine numbered `engine`.
    #[must_use]
    pub const fn new(engine: u32, bits: u64) -> Self {
        Self { engine, bits }
    }

    /// The engine that holds the blob.
    #[must_use]
    pub const fn engine(self) -> u32 {
        self.engine
    }

    /// Its number in that engine.
    #[must_use]
    pub const fn bits(self) -> u64 {
        self.bits
    }

    /// The id a job gives the `index`th blob it puts in the engine numbered `engine`: under a writer no engine has, so
    /// it names no blob the engine holds, and a change to its pins changes nothing (ADR-0018).
    #[must_use]
    pub const fn made(engine: u32, index: u64) -> Self {
        Self { engine, bits: MADE << PUT_BITS | index }
    }

    /// Which of the blobs a job put it names, when a job gave it.
    #[must_use]
    pub const fn made_index(self) -> Option<u64> {
        let is_made = self.bits >> PUT_BITS == MADE;
        if is_made {
            Some(self.bits & ((1 << PUT_BITS) - 1))
        } else {
            None
        }
    }
}

/// A blob's content hash: BLAKE3 over its bytes, computed the first time something asks for it and kept with the
/// blob (ADR-0014). Two blobs of the same bytes have the same hash, in any engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlobHash([u8; 32]);

impl BlobHash {
    /// The hash of `bytes`.
    #[cfg(feature = "std")]
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        Self(*blake3::hash(bytes).as_bytes())
    }

    /// The hash whose 32 bytes are `bytes`: for a guest, whose host hashed the blob (ADR-0015).
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Its 32 bytes.
    #[must_use]
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }
}

/// How many bits of a blob's id count the puts of its writer: the bits above them name the writer.
const PUT_BITS: u32 = 48;

/// The writer a job's own blobs and resources are numbered under, which no engine's writers reach (ADR-0018).
const MADE: u64 = (1 << (u64::BITS - PUT_BITS)) - 1;

/// The ticket a job gives the `index`th resource it hands: under the writer its blobs are numbered under, so it names
/// no resource the engine keeps until the engine writes the resource's own ticket over it (ADR-0018).
#[must_use]
pub const fn handed_ticket(index: u64) -> u64 {
    MADE << PUT_BITS | index
}

/// Which of the resources a job handed `ticket` names, when a job gave it.
#[must_use]
pub const fn handed_index(ticket: u64) -> Option<u64> {
    let is_handed = ticket >> PUT_BITS == MADE;
    if is_handed {
        Some(ticket & ((1 << PUT_BITS) - 1))
    } else {
        None
    }
}

/// The blobs one writer has put in a step, held until the step folds, the hashes its turns computed in the step, and
/// the room it has left in the step's share of the budget for blobs (ADR-0014); and how many resources its turns have
/// kept (ADR-0010).
///
/// The engine keeps one for each writer from step to step, so the puts it counts give each blob an id no other blob
/// has had, and the resources it counts each a ticket no other resource of the engine has had.
#[derive(Debug)]
#[cfg_attr(not(feature = "std"), expect(dead_code, reason = "blobs are put only where their pins can be noted"))]
pub struct Puts {
    /// The engine the writer writes for.
    engine: u32,
    /// The writer, in the high bits of each id it gives.
    writer: u64,
    /// How many blobs the writer has put, ever.
    made: u64,
    /// How many resources the writer's turns have kept, ever.
    kept: u64,
    /// The bytes the writer may still put in this step.
    room: usize,
    /// What it has put in this step, under its ids' bits.
    held: BTreeMap<u64, Box<[u8]>>,
    /// Who takes in what it puts.
    putter: Putter,
    /// The hashes its turns computed in this step, of blobs it put or the store holds, for the store to keep. Two
    /// writers, or two turns, may hash one blob in a step, and the store keeps the one hash they all compute.
    hashed: Vec<(BlobId, BlobHash)>,
}

impl Puts {
    /// The puts of the writer numbered `writer` in the engine numbered `engine`, with `room` bytes to put.
    ///
    /// # Panics
    ///
    /// Panics for a writer numbered past what an id's high bits hold, which no engine has.
    #[must_use]
    pub fn new(engine: u32, writer: usize, room: usize) -> Self {
        let Ok(writer) = u64::try_from(writer) else {
            panic!("no engine has more writers than an id's high bits number");
        };
        assert!(writer < MADE, "no engine has more writers than an id's high bits number");

        let held = BTreeMap::new();

        Self { engine, writer, made: 0, kept: 0, room, held, hashed: Vec::new(), putter: Putter::Writer }
    }

    /// Puts a guest's host takes in, in a call out, for a guest whose turns cannot reach the engine's store (ADR-0015).
    #[must_use]
    pub fn of_host(put: fn(Vec<u8>) -> Result<BlobId, Refused>) -> Self {
        Self { putter: Putter::Host(put), ..Self::new(0, 0, 0) }
    }

    /// Whether a guest's host takes them in.
    #[must_use]
    pub const fn is_of_host(&self) -> bool {
        matches!(self.putter, Putter::Host(_))
    }

    /// A ticket for a resource kept, which no other resource of the engine has had: the writer in its high bits, and
    /// its count of what it has kept below (ADR-0010).
    ///
    /// # Panics
    ///
    /// Panics past `2^48` resources kept by one writer, which no engine lives to keep.
    pub fn ticket(&mut self) -> u64 {
        assert!(self.kept + 1 < 1 << PUT_BITS, "no writer keeps 2^48 resources");
        self.kept += 1;

        self.writer << PUT_BITS | self.kept
    }

    /// Gives the writer `room` bytes to put in the coming step.
    pub const fn give_room(&mut self, room: usize) {
        self.room = room;
    }

    /// The bytes of the blob `id` names, when this writer put it in this step.
    #[must_use]
    pub fn read(&self, id: BlobId) -> Option<&[u8]> {
        self.held.get(&id.bits()).map(|bytes| &bytes[..])
    }

    /// Notes that the blob `id` names has the hash `hash`, for the store to keep.
    pub fn hashed(&mut self, id: BlobId, hash: BlobHash) {
        self.hashed.push((id, hash));
    }

    /// Takes the hashes computed in this step, each with its blob's id, for the store to keep, leaving none.
    pub fn take_hashed(&mut self) -> impl Iterator<Item = (BlobId, BlobHash)> + '_ {
        self.hashed.drain(..)
    }

    /// Takes the blobs put in this step, each with its id, for the store to take in, leaving none held.
    pub fn take(&mut self) -> impl Iterator<Item = (BlobId, Box<[u8]>)> + '_ {
        let held = mem::take(&mut self.held);

        let engine = self.engine;

        held.into_iter().map(move |(bits, bytes)| (BlobId::new(engine, bits), bytes))
    }

    /// Holds `bytes` as a new blob and answers with its id: refused when they would take the writer past its room.
    ///
    /// # Panics
    ///
    /// Panics past `2^48` puts by one writer, which no engine lives to make.
    #[cfg_attr(not(feature = "std"), expect(dead_code, reason = "blobs are put only where their pins can be noted"))]
    fn put(&mut self, bytes: Vec<u8>) -> Result<BlobId, Refused> {
        if let Putter::Host(put) = self.putter {
            return put(bytes);
        }
        let fits = bytes.len() <= self.room;
        if !fits {
            return Err(Refused::NoRoom);
        }
        assert!(self.made < 1 << PUT_BITS, "no writer puts 2^48 blobs");
        self.room -= bytes.len();
        let id = BlobId::new(self.engine, self.writer << PUT_BITS | self.made);
        self.made += 1;
        self.held.insert(id.bits(), bytes.into_boxed_slice());

        Ok(id)
    }
}

/// Who takes in what one writer puts.
#[derive(Debug, Clone, Copy)]
enum Putter {
    /// The writer itself, for the store to take in as its step folds.
    Writer,
    /// A guest's host, in a call out, which answers with the blob's id.
    Host(fn(Vec<u8>) -> Result<BlobId, Refused>),
}

#[cfg(feature = "std")]
pub use pinned::{BlobRef, Pinning, pins, within};

#[cfg(feature = "std")]
mod pinned {
    use super::{BlobId, Puts};
    use crate::Refused;
    use crate::{Actors, Within};
    use aethereal_data::{Decode, DecodeError, Encode, EncodeError, Field, Reader, Schema, Shape, Writer};
    use alloc::vec::Vec;

    /// A pin on one blob: while any reference to a blob is held, the blob is kept (ADR-0014).
    ///
    /// A clone is one more pin, and a drop takes one away. Neither changes a
    /// count other threads read: each is noted in a buffer its thread keeps, and
    /// the engine sums the notes when its step folds.
    ///
    /// Only this crate makes one from an id: a put, a clone, and a read out of
    /// bytes that hold its pin are the only ways one comes to be (ADR-0014). It
    /// is laid out as its id, so code that must stand in for a guest that
    /// does not keep the rules, as a test's does, forges one only by `unsafe`.
    #[derive(Debug, PartialEq, Eq)]
    #[repr(transparent)]
    pub struct BlobRef {
        id: BlobId,
    }

    impl BlobRef {
        /// A new pin on the blob `id` names.
        ///
        /// For the engine, which pins a blob as it is put and as a reference to it is read out of bytes that hold
        /// its pin: no code outside it names a blob it was not given.
        #[must_use]
        pub(crate) fn pin(id: BlobId) -> Self {
            pins::change(id, 1);

            Self { id }
        }

        /// The blob it pins.
        #[must_use]
        pub const fn id(&self) -> BlobId {
            self.id
        }
    }

    /// A reference is its blob's id in bytes.
    impl Schema for BlobRef {
        const SHAPE: Shape = Shape::Struct {
            name: "BlobRef",
            fields: &[Field { name: "engine", shape: u32::SHAPE }, Field { name: "id", shape: u64::SHAPE }],
        };
    }

    /// An actor's state holds its references as their ids, and the bytes keep a pin on each one's blob beside them
    /// (ADR-0014). One read back out of them is a copy of the pin they keep, so it is a pin of its own on a blob that
    /// was kept; a reference to a blob they keep no pin on is refused.
    impl Encode<Within> for BlobRef {
        fn encode(&self, out: &mut Writer<'_, Within>) -> Result<(), EncodeError> {
            out.offers().hold(self.clone());
            self.id.engine().encode(out)?;
            self.id.bits().encode(out)
        }
    }

    impl Decode<Within> for BlobRef {
        fn decode(input: &mut Reader<'_, Within>) -> Result<Self, DecodeError> {
            let engine = u32::decode(input)?;
            let id = BlobId::new(engine, u64::decode(input)?);

            input.offers().pin(id).ok_or(DecodeError::Unheld)
        }
    }

    /// A reference in mail is its blob's id in bytes, and the mail holds a pin of its own while it is in flight:
    /// the writer keeps it, at where the reference's bytes begin, beside the bytes it writes (ADR-0014). The receiver
    /// that reads it back takes a pin of its own, and the mail's goes with the mail.
    impl Encode<Actors> for BlobRef {
        fn encode(&self, out: &mut Writer<'_, Actors>) -> Result<(), EncodeError> {
            let at = u32::try_from(out.bytes().len()).unwrap_or(u32::MAX);
            out.offers().hold(at, self.clone());
            self.id.engine().encode(out)?;
            self.id.bits().encode(out)
        }
    }

    /// The receiver's reference is a copy of the pin the mail holds at where the reference begins; one with no pin
    /// there is refused.
    impl Decode<Actors> for BlobRef {
        fn decode(input: &mut Reader<'_, Actors>) -> Result<Self, DecodeError> {
            let at = input.input().as_ptr().addr();
            let engine = u32::decode(input)?;
            let id = BlobId::new(engine, u64::decode(input)?);

            input.offers().pin(at, id).ok_or(DecodeError::Unheld)
        }
    }

    /// A pin some bytes hold, and where in them the reference it is for begins.
    pub type Pinning = (u32, BlobRef);

    /// The pins among `held`, kept in the order of where they lie, that lie within the `len` bytes from `start`.
    #[must_use]
    pub fn within(held: &[Pinning], start: u32, len: u32) -> &[Pinning] {
        let end = start.saturating_add(len);
        let first = held.partition_point(|(at, _)| *at < start);
        let past = held.partition_point(|(at, _)| *at < end);

        &held[first..past]
    }

    impl Clone for BlobRef {
        fn clone(&self) -> Self {
            Self::pin(self.id)
        }
    }

    impl Drop for BlobRef {
        fn drop(&mut self) {
            pins::change(self.id, -1);
        }
    }

    impl Puts {
        /// Puts `bytes` as a new blob and answers with the first pin on it: refused when they would take the writer
        /// past its room.
        ///
        /// # Errors
        ///
        /// [`Refused::NoRoom`] when the bytes would take the writer past its room.
        pub fn pinned(&mut self, bytes: Vec<u8>) -> Result<BlobRef, Refused> {
            self.put(bytes).map(BlobRef::pin)
        }
    }

    /// Where the changes to blobs' pins are noted until the engine sums them (ADR-0014).
    ///
    /// A thread the engine runs a step's turns on opens a buffer of its own
    /// for them, and takes it back when they are done. A thread that runs a job
    /// pushes them onto its engine's queue while it runs the job (ADR-0018). A
    /// change made on any other thread, or on one with no buffer open, goes to
    /// one buffer they share.
    pub mod pins {
        use crate::BlobId;
        use alloc::sync::Arc;
        use alloc::vec::Vec;
        use core::cell::RefCell;
        use core::mem;
        use crossbeam_queue::SegQueue;
        use std::collections::BTreeMap;
        use std::sync::{Mutex, PoisonError};

        /// One change to one blob's pins: one more, or one fewer.
        pub type Change = (BlobId, i64);

        /// Where the changes to one engine's pins made while its jobs run go: a queue no one waits on, so a thread
        /// running a job never waits for another, nor for the engine, and the engine never waits for it (ADR-0018).
        pub type Queue = Arc<SegQueue<Change>>;

        /// Whether this thread has a buffer of its own open, and for which engine.
        enum Buffer {
            Closed,
            Open {
                engine: u32,
                changes: Vec<Change>,
            },
            /// Open for the changes of every engine: a guest's, which holds references of whatever engine its host
            /// runs in (ADR-0015).
            #[cfg(feature = "guest")]
            Every {
                changes: Vec<Change>,
            },
            /// Pushing the changes of one engine onto its queue, while a job of it runs.
            Queued {
                engine: u32,
                queue: Queue,
            },
        }

        std::thread_local! {
            static OWN: RefCell<Buffer> = const { RefCell::new(Buffer::Closed) };
        }

        /// The changes made where no buffer for their engine was open, by engine.
        static SHARED: Mutex<BTreeMap<u32, Vec<Change>>> = Mutex::new(BTreeMap::new());

        /// Notes that the blob `id` names has `by` more pins.
        pub(crate) fn change(id: BlobId, by: i64) {
            // A thread being torn down has no buffer left to borrow, and its changes go where unbuffered ones do.
            let noted = OWN.try_with(|own| match &mut *own.borrow_mut() {
                Buffer::Open { engine, changes } if *engine == id.engine() => {
                    changes.push((id, by));
                    true
                }
                #[cfg(feature = "guest")]
                Buffer::Every { changes } => {
                    changes.push((id, by));
                    true
                }
                Buffer::Queued { engine, queue } if *engine == id.engine() => {
                    queue.push((id, by));
                    true
                }
                Buffer::Open { .. } | Buffer::Queued { .. } | Buffer::Closed => false,
            });
            let is_noted = noted.unwrap_or(false);
            if !is_noted {
                SHARED.lock().unwrap_or_else(PoisonError::into_inner).entry(id.engine()).or_default().push((id, by));
            }
        }

        /// Opens a buffer of this thread's own for the changes to the pins of the engine numbered `engine`, made on it
        /// until [`close`]: `room`, emptied and kept for its room.
        pub fn open(engine: u32, room: Vec<Change>) {
            debug_assert!(room.is_empty(), "a buffer is opened empty");
            OWN.with(|own| *own.borrow_mut() = Buffer::Open { engine, changes: room });
        }

        /// Closes this thread's buffer and answers with the changes made on it since [`open`]: none when it had no
        /// buffer open.
        #[must_use]
        pub fn close() -> Vec<Change> {
            OWN.with(|own| match mem::replace(&mut *own.borrow_mut(), Buffer::Closed) {
                Buffer::Open { changes, .. } => changes,
                #[cfg(feature = "guest")]
                Buffer::Every { changes } => changes,
                Buffer::Queued { .. } | Buffer::Closed => Vec::new(),
            })
        }

        /// Runs `run` with the changes to the pins of the engine numbered `engine` made on this thread pushed onto
        /// `queue`, then puts back the buffer this thread had, even when `run` panics.
        pub fn queued<R>(engine: u32, queue: &Queue, run: impl FnOnce() -> R) -> R {
            let queued = Buffer::Queued { engine, queue: Arc::clone(queue) };
            let had = OWN.with(|own| mem::replace(&mut *own.borrow_mut(), queued));
            let _back = PutBack(had);

            run()
        }

        /// The buffer a thread had before it ran a job, put back once the job has run.
        struct PutBack(Buffer);

        impl Drop for PutBack {
            fn drop(&mut self) {
                let had = mem::replace(&mut self.0, Buffer::Closed);
                // A thread being torn down has no buffer left to put back.
                let _ = OWN.try_with(|own| *own.borrow_mut() = had);
            }
        }

        /// Takes the changes `queue` holds as this begins, adding them to `into`: what is pushed while it takes waits
        /// for the next, so taking is bounded however fast jobs push.
        pub fn take_queued(queue: &Queue, into: &mut Vec<Change>) {
            let ready = queue.len();
            into.extend((0..ready).map_while(|_| queue.pop()));
        }

        /// Takes the changes this thread has made since it last did, adding them to `into`, and keeps its buffer open
        /// for those of every engine: for a guest's library, which hands its changes to its host after each call
        /// (ADR-0014, ADR-0015). Changes made before its buffer was first open went where unbuffered ones go.
        #[cfg(feature = "guest")]
        pub fn take_own(into: &mut Vec<Change>) {
            OWN.with(|own| match &mut *own.borrow_mut() {
                Buffer::Every { changes } => into.append(changes),
                buffer => *buffer = Buffer::Every { changes: Vec::new() },
            });
        }

        /// Takes the changes to the pins of the engine numbered `engine` made where no buffer for it was open, adding
        /// them to `into`.
        pub fn take_shared(engine: u32, into: &mut Vec<Change>) {
            let mut shared = SHARED.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(changes) = shared.get_mut(&engine) {
                into.append(changes);
            }
        }

        /// Forgets the changes left for the engine numbered `engine`, once it has gone.
        pub fn forget(engine: u32) {
            SHARED.lock().unwrap_or_else(PoisonError::into_inner).remove(&engine);
        }
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::{
        Actors, ActorsWriter, Mail, MailId, PinSource, Pinned, Pinning, SeatNumber, Seated, Unanswered, Written,
    };
    use aethereal_data::{ActorId, DecodeError, Encode, KindId, Writer};

    #[test]
    fn bytes_written_keep_their_blobs_and_each_reference_read_back_is_a_pin_of_its_own() {
        // Catches bytes that keep no pin of their own, so the blob is freed once the reference they were written from
        // is dropped; and a reference read back with no pin of its own, freed once the bytes go.
        let id = BlobId::new(7, 3);
        pins::open(7, Vec::new());
        let blob = BlobRef::pin(id);
        let Ok(written) = Written::of(&blob) else {
            panic!("a reference is written");
        };
        drop(blob);
        let while_kept: i64 = pins::close().iter().map(|(_, by)| by).sum();
        pins::open(7, Vec::new());
        let read = written.read::<BlobRef>(64);
        drop(written);
        let changes = pins::close();

        assert_eq!(while_kept, 1, "the bytes keep the blob");
        assert_eq!(read.as_ref().map(BlobRef::id), Ok(id));
        assert_eq!(changes.iter().map(|(_, by)| by).sum::<i64>(), 0, "the reference read back keeps its pin");
    }

    #[test]
    fn a_reference_in_bytes_that_keep_no_pin_on_its_blob_is_refused() {
        // Catches a reference read back from bytes that do not keep its blob, which would pin a blob that may have
        // been freed.
        pins::open(8, Vec::new());
        let blob = BlobRef::pin(BlobId::new(8, 3));
        let bytes = Written::of(&blob).map(|written| written.bytes().to_vec()).unwrap_or_default();
        let read = Written::new(bytes, Vec::new()).read::<BlobRef>(64);
        drop(blob);
        let _ = pins::close();

        assert_eq!(read, Err(DecodeError::Unheld));
    }

    /// `blob` written as mail, and the pins its writer kept beside the bytes, each at where its reference begins.
    fn mailed(blob: &BlobRef) -> (Vec<u8>, Vec<Pinning>) {
        let mut held = Vec::new();
        let mut out = Writer::<Actors>::new(ActorsWriter::new(&mut held));
        assert_eq!(blob.encode(&mut out), Ok(()));

        (out.into_bytes(), held)
    }

    /// The reference read out of mail of `bytes` that hold the pins `held` says: as they lie where they were
    /// written, and as a hop's, copied out of the arena they were written in.
    fn read_mailed(bytes: &[u8], held: &[Pinning]) -> [Result<BlobRef, DecodeError>; 2] {
        let sender = Seated { id: ActorId::new(1), seat: SeatNumber::new(0) };
        let root = MailId { sender: sender.id, count: 1 };
        let sources = [PinSource::lying(bytes, held), PinSource::copied(bytes, Pinned::lying(held, 0))];

        sources.map(|pins| Mail::new(KindId::of::<Unanswered>(), bytes, &pins, sender, root, 1, 64).contents())
    }

    #[test]
    fn a_reference_read_out_of_mail_is_a_copy_of_the_pin_the_mail_holds() {
        // Catches a receiver's reference taken with no pin of its own, freed once the mail's arena is emptied.
        let id = BlobId::new(9, 3);
        pins::open(9, Vec::new());
        let blob = BlobRef::pin(id);
        let (bytes, held) = mailed(&blob);
        drop(blob);
        let read = read_mailed(&bytes, &held);
        drop(held);
        let changes = pins::close();

        assert_eq!(read.each_ref().map(|read| read.as_ref().map(BlobRef::id)), [Ok(id); 2]);
        assert_eq!(changes.iter().map(|(_, by)| by).sum::<i64>(), 2, "each reference read back keeps its pin");
    }

    #[test]
    fn a_reference_in_mail_with_no_pin_where_it_begins_is_refused() {
        // Catches a reference read out of mail on its bytes alone: mail that holds no pin, one at another place in
        // the bytes, or one on another blob, would each have it pin a blob that may have been freed.
        pins::open(10, Vec::new());
        let (blob, other) = (BlobRef::pin(BlobId::new(10, 3)), BlobRef::pin(BlobId::new(10, 4)));
        let (bytes, held) = mailed(&blob);
        let elsewhere = [(4, blob.clone())];
        let another = [(0, other.clone())];

        let refused = [read_mailed(&bytes, &[]), read_mailed(&bytes, &elsewhere), read_mailed(&bytes, &another)];

        drop((blob, other, held, elsewhere, another));
        let _ = pins::close();
        assert_eq!(refused.map(|read| read.map(Result::err)), [[Some(DecodeError::Unheld); 2]; 3]);
    }
}
