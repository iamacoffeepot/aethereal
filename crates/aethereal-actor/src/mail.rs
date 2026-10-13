//! A piece of mail as its handler sees it (ADR-0009).

use crate::{ActorRef, Actors, ActorsReader, Table};
use aethereal_data::{ActorId, Budget, Decode, DecodeError, Kind, KindId, Reader};

/// A piece of mail as its handler sees it: its kind, its sender, and its bytes to read (ADR-0009).
#[derive(Clone, Copy)]
pub struct Mail<'a> {
    kind: KindId,
    bytes: &'a [u8],
    sender: ActorId,
    table: &'a dyn Table,
    limit: usize,
}

impl<'a> Mail<'a> {
    /// The mail of kind `kind` whose bytes are `bytes`, sent by the actor `sender` stands for.
    ///
    /// For the engine, which has established that the sender exists. A read proves each reference in the mail
    /// against `table`, and allocates no more than `limit` bytes.
    #[must_use]
    pub const fn new(kind: KindId, bytes: &'a [u8], sender: ActorId, table: &'a dyn Table, limit: usize) -> Self {
        Self { kind, bytes, sender, table, limit }
    }

    /// The id of the mail's kind.
    #[must_use]
    pub const fn kind(&self) -> KindId {
        self.kind
    }

    /// The actor that sent the mail. Every piece of mail has one.
    #[must_use]
    pub const fn sender(&self) -> ActorRef {
        ActorRef::new(self.sender)
    }

    /// The mail's contents, when it is of kind `K`; nothing when it is of another kind.
    ///
    /// Mail can arrive as bytes a guest wrote, so bytes that do not decode as the kind they claim are an error.
    pub fn read<K: Kind + Decode<Actors>>(&self) -> Result<Option<K>, DecodeError> {
        let is_kind = self.kind == KindId::of::<K>();
        if !is_kind {
            return Ok(None);
        }

        let mut budget = Budget::new(self.limit);
        let mut reader = Reader::<Actors>::new(self.bytes, &mut budget, ActorsReader::new(self.table));
        let contents = K::decode(&mut reader)?;
        let trailing = reader.input().len();
        let is_whole = trailing == 0;
        if is_whole {
            Ok(Some(contents))
        } else {
            Err(DecodeError::TrailingBytes { count: trailing })
        }
    }
}
