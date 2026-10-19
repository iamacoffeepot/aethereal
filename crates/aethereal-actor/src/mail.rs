//! A piece of mail as its handler sees it (ADR-0009).

use crate::{Actors, ActorsReader, ErasedActorRef, Seated};
use aethereal_data::{ActorId, Budget, Decode, DecodeError, Kind, KindId, Plain, Reader};

/// The id of a piece of mail: its sender, and that sender's count of what it has sent (ADR-0012).
///
/// Each actor counts only its own mail, from one, so no count is shared
/// between actors, and the same turns give every piece of mail the same id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Plain)]
pub struct MailId {
    /// The actor that sent the mail.
    pub sender: ActorId,
    /// The sender's own count of what it has sent, from one.
    pub count: u64,
}

/// A piece of mail as its handler sees it: its kind, its sender, and its bytes to read (ADR-0009).
#[derive(Clone, Copy)]
pub struct Mail<'a> {
    kind: KindId,
    bytes: &'a [u8],
    /// Where the pins the bytes hold are found (ADR-0014).
    #[cfg(feature = "std")]
    pins: &'a crate::PinSource<'a>,
    sender: Seated,
    root: MailId,
    depth: u32,
    limit: usize,
}

impl<'a> Mail<'a> {
    /// The mail of kind `kind` whose bytes are `bytes`, whose pins `pins` finds, sent by the actor `sender` names, at
    /// `depth` in the chain that began at `root`.
    ///
    /// For the engine, which has established that the sender exists. A read allocates no more than `limit` bytes, and
    /// takes each reference as a copy of a pin `pins` finds, so a pin it lacks is a reference refused.
    #[cfg(feature = "std")]
    #[must_use]
    pub const fn new(
        kind: KindId,
        bytes: &'a [u8],
        pins: &'a crate::PinSource<'a>,
        sender: Seated,
        root: MailId,
        depth: u32,
        limit: usize,
    ) -> Self {
        Self { kind, bytes, pins, sender, root, depth, limit }
    }

    /// The mail of kind `kind` whose bytes are `bytes`, sent by the actor `sender` names, at `depth` in the chain
    /// that began at `root`, where no blob is referred to.
    #[cfg(not(feature = "std"))]
    #[must_use]
    pub const fn new(kind: KindId, bytes: &'a [u8], sender: Seated, root: MailId, depth: u32, limit: usize) -> Self {
        Self { kind, bytes, sender, root, depth, limit }
    }

    /// The pins the mail's bytes hold, each at where its reference begins: for a host, which offers them to the code
    /// it hands the bytes to (ADR-0014).
    #[cfg(feature = "std")]
    #[must_use]
    pub fn pinned(&self) -> crate::Pinned<'a> {
        self.pins.pinned(self.bytes)
    }

    /// The most bytes a read of it allocates.
    pub(crate) const fn limit(&self) -> usize {
        self.limit
    }

    /// The id of the mail's kind.
    #[must_use]
    pub const fn kind(&self) -> KindId {
        self.kind
    }

    /// The mail's bytes, as written: for a host, which hands them to a guest that reads them itself (ADR-0015).
    #[must_use]
    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// The seat of the actor that sent the mail, with its id.
    #[must_use]
    pub const fn sender_seated(&self) -> Seated {
        self.sender
    }

    /// The actor that sent the mail. Every piece of mail has one.
    #[must_use]
    pub const fn sender(&self) -> ErasedActorRef {
        ErasedActorRef::new(self.sender)
    }

    /// The first mail of the chain this mail belongs to (ADR-0012).
    #[must_use]
    pub const fn root(&self) -> MailId {
        self.root
    }

    /// How far down its chain this mail is: one more than the mail whose turn sent it (ADR-0012).
    ///
    /// Mail an actor sends while handling this is one deeper. An actor that keeps something under a chain's root
    /// and the depth it sent at finds it again from mail further down that chain, and reads how far back it was.
    #[must_use]
    pub const fn depth(&self) -> u32 {
        self.depth
    }

    /// The mail's contents, when it is of kind `K`; nothing when it is of another kind.
    ///
    /// Mail can arrive as bytes a guest wrote, so bytes that do not decode as the kind they claim are an error.
    pub fn read<K: Kind + Decode<Actors>>(&self) -> Result<Option<K>, DecodeError> {
        let is_kind = self.kind == KindId::of::<K>();
        if !is_kind {
            return Ok(None);
        }

        self.contents().map(Some)
    }

    /// The mail's contents, read as `K`, whose handler it was handed to: the kind is already known to match.
    pub(crate) fn contents<K: Decode<Actors>>(&self) -> Result<K, DecodeError> {
        let mut budget = Budget::new(self.limit);
        #[cfg(feature = "std")]
        let offers = ActorsReader::of(self.pins);
        #[cfg(not(feature = "std"))]
        let offers = ActorsReader::of();
        let mut reader = Reader::<Actors>::new(self.bytes, &mut budget, offers);
        let contents = K::decode(&mut reader)?;
        let trailing = reader.input().len();
        let is_whole = trailing == 0;
        if is_whole {
            Ok(contents)
        } else {
            Err(DecodeError::TrailingBytes { count: trailing })
        }
    }
}
