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
}

/// How many bits of a blob's id count the puts of its writer: the bits above them name the writer.
const PUT_BITS: u32 = 48;

/// The blobs one writer has put in a step, held until the step folds, and the room it has left in the step's share of
/// the budget for blobs (ADR-0014).
///
/// The engine keeps one for each writer from step to step, so the puts it counts give each blob an id no other blob
/// has had.
#[derive(Debug)]
#[cfg_attr(not(feature = "std"), expect(dead_code, reason = "blobs are put only where their pins can be noted"))]
pub struct Puts {
    /// The engine the writer writes for.
    engine: u32,
    /// The writer, in the high bits of each id it gives.
    writer: u64,
    /// How many blobs the writer has put, ever.
    made: u64,
    /// The bytes the writer may still put in this step.
    room: usize,
    /// What it has put in this step, under its ids' bits.
    held: BTreeMap<u64, Box<[u8]>>,
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
        assert!(writer < 1 << (u64::BITS - PUT_BITS), "no engine has more writers than an id's high bits number");

        Self { engine, writer, made: 0, room, held: BTreeMap::new() }
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

#[cfg(feature = "std")]
pub use pinned::{BlobRef, pins};

#[cfg(feature = "std")]
mod pinned {
    use super::{BlobId, Puts};
    use crate::Refused;
    use crate::Within;
    use aethereal_data::{Decode, DecodeError, Encode, EncodeError, Field, Reader, Schema, Shape, Writer};
    use alloc::vec::Vec;

    /// A pin on one blob: while any reference to a blob is held, the blob is kept (ADR-0014).
    ///
    /// A clone is one more pin, and a drop takes one away. Neither changes a
    /// count other threads read: each is noted in a buffer its thread keeps, and
    /// the engine sums the notes when its step folds.
    #[derive(Debug, PartialEq, Eq)]
    pub struct BlobRef {
        id: BlobId,
    }

    impl BlobRef {
        /// A new pin on the blob `id` names.
        ///
        /// For the engine, which pins a blob as it is put and as a reference to it is read out of bytes.
        #[must_use]
        pub fn pin(id: BlobId) -> Self {
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

    /// An actor's state holds its references as their ids. One read back out of that state is a pin of its own, as
    /// the state it was written from still holds its own.
    impl Encode<Within> for BlobRef {
        fn encode(&self, out: &mut Writer<'_, Within>) -> Result<(), EncodeError> {
            self.id.engine().encode(out)?;
            self.id.bits().encode(out)
        }
    }

    impl Decode<Within> for BlobRef {
        fn decode(input: &mut Reader<'_, Within>) -> Result<Self, DecodeError> {
            let engine = u32::decode(input)?;
            let id = BlobId::new(engine, u64::decode(input)?);

            Ok(Self::pin(id))
        }
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
    /// for them, and takes it back when they are done. A change made on any other
    /// thread, or on one with no buffer open, goes to one buffer they share.
    pub mod pins {
        use crate::BlobId;
        use alloc::vec::Vec;
        use core::cell::RefCell;
        use core::mem;
        use std::collections::BTreeMap;
        use std::sync::{Mutex, PoisonError};

        /// One change to one blob's pins: one more, or one fewer.
        pub type Change = (BlobId, i64);

        /// Whether this thread has a buffer of its own open, and for which engine.
        enum Buffer {
            Closed,
            Open { engine: u32, changes: Vec<Change> },
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
                Buffer::Open { .. } | Buffer::Closed => false,
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
                Buffer::Closed => Vec::new(),
            })
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
    use crate::{Within, WithinReader, WithinWriter};
    use aethereal_data::{Budget, Decode, Encode, Reader, Writer};

    #[test]
    fn a_reference_read_out_of_an_actors_state_is_a_pin_of_its_own() {
        // Catches a reference read back with no pin of its own, so the blob is freed once the one it was written
        // from is dropped, while the one read back still names it.
        let id = BlobId::new(7, 3);
        pins::open(7, Vec::new());
        let written = BlobRef::pin(id);
        let mut out = Writer::<Within>::new(WithinWriter::new());
        assert_eq!(written.encode(&mut out), Ok(()));
        let bytes = out.into_bytes();
        let mut budget = Budget::new(64);
        let read = BlobRef::decode(&mut Reader::<Within>::new(&bytes, &mut budget, WithinReader::new()));
        drop(written);
        let changes = pins::close();

        assert_eq!(read.as_ref().map(BlobRef::id), Ok(id));
        assert_eq!(changes.iter().map(|(_, by)| by).sum::<i64>(), 1, "the reference read back keeps its pin");
    }
}
