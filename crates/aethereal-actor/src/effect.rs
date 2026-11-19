//! What an actor asks of the engine, in three lists a turn fills as it runs: the mail it sends, the lines it logs,
//! and the rest, its effects on the lives of actors (ADR-0009).

#[cfg(feature = "std")]
use crate::Bytes;
use crate::{BlobId, Declared, Due, ErasedActorRef, Fault, How, Payload, SeatNumber, Seated, SentAs, Unreadable};
use aethereal_data::{ActorId, KeyName, KindId, KindName, PathSegment};

/// One thing an actor asks of the engine, and which actor asked (ADR-0009).
///
/// A turn returns a list of these, and nothing a turn asks for happens during
/// the turn. A view is folded from them (ADR-0011).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effect {
    /// The actor that asked, and its seat.
    by: Seated,
    what: What,
}

impl Effect {
    /// The effect by which the actor `by` names asks for `what`.
    #[must_use]
    pub const fn new(by: Seated, what: What) -> Self {
        Self { by, what }
    }

    /// The id of the actor that asked.
    #[must_use]
    pub const fn by(&self) -> ActorId {
        self.by.id
    }

    /// The actor that asked, and its seat.
    #[must_use]
    pub const fn seated(&self) -> Seated {
        self.by
    }

    /// What it asked for.
    #[must_use]
    pub const fn what(&self) -> &What {
        &self.what
    }

    /// What it asked for, taken from the effect.
    #[must_use]
    pub fn into_what(self) -> What {
        self.what
    }
}

/// What an effect asks for (ADR-0009).
///
/// The cases here are the ones a turn can ask for so far. The others ADR-0009
/// lists enter with the piece of the engine that applies each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum What {
    /// A root, a singleton, has been made. Written by the engine for the chassis, by the root itself.
    Root { from: Code },
    /// A root of an instanced type has been made. Written by the engine for the chassis, by the root itself.
    RootInstanced { from: Code, key: KeyName },
    /// A child, a singleton, stands beneath the asking actor in `seat` and takes its own mail.
    Spawn { from: Code, seat: SeatNumber },
    /// A child of an instanced type stands beneath the asking actor in `seat` and takes its own mail.
    SpawnInstanced { from: Code, key: KeyName, seat: SeatNumber },
    /// The asking actor's turn failed with this fault, and the actor has ended. Written by the engine, since the
    /// actor asked for nothing.
    Failed(Fault),
    /// The asking actor's closing turn failed with this fault. Written by the engine. The actor had already
    /// ended, so this marks nothing: it is there for the log.
    EndFailed(Fault),
    /// The asking actor's closing turn has run, and its life is over: this is how it ended. Written by the engine,
    /// with the closing turn's effects, so it comes after the actor's last mail.
    Ended(How),
    /// The asking actor is shown the ending of `of`, once, when that actor ends. `seen` is how the asking turn saw
    /// `of`, so the engine keeps nothing of an actor once it has ended (ADR-0008).
    Watch { of: ErasedActorRef, seen: Seen },
    /// The asking actor closes.
    Close,
    /// The asking actor is sent the stage of the program's lifecycle of this kind, each time it runs (ADR-0020).
    Subscribe(KindId),
    /// The asking actor is no longer sent the stage of this kind.
    Unsubscribe(KindId),
    /// The asking actor asks the program to stop: the engine lets a cycle under way finish, and runs the last stage
    /// (ADR-0020).
    Quit,
    /// The asking actor holds `bytes` unsent, of the most it may hold, `most`: what it said last is the engine's
    /// figure for it until it says again or ends (ADR-0053).
    Unsent { bytes: u64, most: u64 },
    /// The mail the asking actor's turn was handed did not read as its kind, for this reason, and its handler did not
    /// run: the engine answers it [`Unanswered::Unreadable`](crate::Unanswered::Unreadable) when it asks a request, and
    /// otherwise drops it, as the actor's log records (ADR-0017). Written by the engine, or by a guest's host as its
    /// library reports it, since the actor's code asked for nothing.
    Unread(Unreadable),
    /// The head of the kind `kind` is the blob `blob` pins from now on, under the publish numbered `number`, asked with
    /// the engine's one publisher, and checked where it was asked to make that kind (ADR-0009, ADR-0054). The heads
    /// view takes the number as it folds it, and keeps the pin.
    #[cfg(feature = "std")]
    Publish { kind: KindName, blob: Bytes, number: u64 },
    /// The asking actor moved onto its kind's head in the step, and runs code whose type `declared` declares from now
    /// on: its path, seat and place in the table are as they were. Written by the engine for the actor's host, which
    /// moved it before a batch of its turns (ADR-0054).
    Moved { declared: &'static Declared },
}

impl Effect {
    /// Whether it says the mail the actor `by`'s turn was handed did not read (ADR-0017).
    #[must_use]
    pub const fn is_unread_by(&self, by: Seated) -> bool {
        let is_unread = matches!(self.what, What::Unread(_));
        let is_own = self.by.id.bits() == by.id.bits();

        is_unread && is_own
    }
}

/// A piece of mail a turn sends, or a run of them (ADR-0009, ADR-0011).
///
/// Mail of kind `kind`, whose bytes are at `bytes`, in the arena of the worker that ran the turn or in bytes of
/// their own, is sent by the actor `by` names, as the type `sent_as` stands for, to the actor `to` names, in its seat,
/// as the turn resolved the target it was given. `count` is the sender's count of mail sent, which with the sender is
/// the mail's id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mailed {
    pub by: Seated,
    /// The type `by` sent it as, stamped as it was sent, while `by` is live in its turn: what a guard reads of the
    /// sender where the mail is delivered (ADR-0058).
    pub sent_as: SentAs,
    pub to: Seated,
    pub count: u64,
    pub kind: KindId,
    pub bytes: Payload,
    /// What it is to a request: none of one, one, or an answer to one (ADR-0017).
    pub tie: Tie,
    /// When the request it is stops being waited for: `Never` for mail that asks nothing (ADR-0041).
    pub due: Due,
    /// The section of its receiver it is for, which its sender worked out from the mail as it sent it, or the whole
    /// actor (spike: keyed sections).
    pub section: SectionKey,
    /// Of a request, the section of its asker its answer goes back to, which the asker worked out from the context it
    /// keeps, and which the engine stamps on the answer as its `section`, whoever gives it: the receiver now, the
    /// receiver later through a hold, or the engine when none comes. [`SectionKey::WHOLE`] for an asker that is not
    /// split into sections, and for mail that asks nothing (spike: keyed sections, return stamps).
    pub returns: SectionKey,
}

/// Which section of a keyed actor a piece of mail is for, stamped on it at the send by the sender, which knows the
/// receiver's type and so how that type finds the key of the mail (spike: keyed sections). The engine routes on the
/// stamp and never reads the mail to find a key.
///
/// It is eight bytes, as a [`Tie`] is: the top bit marks a key, so a key is any number below `2^63`, and the rest is
/// the key. Mail for an actor that is not split into sections carries [`Self::WHOLE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SectionKey(u64);

/// The bit that marks a stamp as a key.
const KEYED: u64 = 1 << 63;

impl SectionKey {
    /// The whole actor: mail for an actor that is not split into sections.
    pub const WHOLE: Self = Self(0);

    /// The section `key` names, its top bit dropped.
    #[must_use]
    pub const fn of(key: u64) -> Self {
        Self(key | KEYED)
    }

    /// Whether it names a section.
    #[must_use]
    pub const fn is_keyed(self) -> bool {
        self.0 & KEYED != 0
    }

    /// The key of the section it names: nought for the whole actor.
    #[must_use]
    pub const fn key(self) -> u64 {
        self.0 & !KEYED
    }

    /// Its eight bytes.
    #[must_use]
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// The stamp whose eight bytes are `bits`, as [`Self::bits`] gives them.
    #[must_use]
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }
}

/// The two stamps a turn of a keyed actor carries: the section the turn runs for, and the section of the asker of the
/// request it handles that the request's answer goes back to, its return stamp. Both [`SectionKey::WHOLE`] for a turn
/// of an actor that is not split, of mail that asks nothing (spike: keyed sections).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamps {
    /// The section of the actor whose turn it is.
    pub section: SectionKey,
    /// The return stamp of the request the turn handles.
    pub returns: SectionKey,
}

impl Stamps {
    /// A turn of the whole actor, of mail whose asker is a whole actor or that asks nothing.
    pub const WHOLE: Self = Self { section: SectionKey::WHOLE, returns: SectionKey::WHOLE };
}

/// What a piece of mail is to a request, in the eight bytes every piece carries (ADR-0017).
///
/// A request names itself by its count, and an answer the request it answers by that request's count: its
/// receiver sent the request, so the count is enough to name it. Counts start at one and stay below the top two
/// bits: the top marks a request, and the next what the engine says of a request that was not answered, which only
/// the engine writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tie(u64);

/// What a [`Tie`] says, read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tied {
    /// The mail is no request, nor an answer to one.
    Tell,
    /// The mail is the request of this count of its sender's.
    Asks(u64),
    /// The mail answers its receiver's request of this count.
    Answers(u64),
    /// The mail is what the engine says of its receiver's request of this count, which was not answered: an
    /// [`Unanswered`](crate::Unanswered).
    Unanswered(u64),
}

/// The bit that marks a request.
const ASKS: u64 = 1 << 63;

/// The bit that marks what the engine says of a request that was not answered.
const UNANSWERED: u64 = 1 << 62;

impl Tie {
    /// Mail that is no request, nor an answer to one.
    pub const TELL: Self = Self(0);

    /// The request that is its sender's `count`th piece of mail.
    #[must_use]
    pub const fn asks(count: u64) -> Self {
        Self(count | ASKS)
    }

    /// The answer to its receiver's request of the count `count`.
    #[must_use]
    pub const fn answers(count: u64) -> Self {
        Self(count)
    }

    /// What the engine says of its receiver's request of the count `count`, which was not answered.
    #[must_use]
    pub const fn unanswered(count: u64) -> Self {
        Self(count | UNANSWERED)
    }

    /// What it says.
    #[must_use]
    pub const fn read(self) -> Tied {
        match self.0 {
            0 => Tied::Tell,
            asks if asks & ASKS != 0 => Tied::Asks(asks & !ASKS),
            unanswered if unanswered & UNANSWERED != 0 => Tied::Unanswered(unanswered & !UNANSWERED),
            count => Tied::Answers(count),
        }
    }

    /// Whether the mail is no request, nor an answer to one.
    #[must_use]
    pub const fn is_tell(self) -> bool {
        self.0 == 0
    }
}

/// A line a turn logs (ADR-0009): by the actor `by` names, at `level`, its text at `text` in the arena of the worker
/// that ran the turn. It is kept when the turn fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line {
    pub by: Seated,
    pub level: Level,
    pub text: Payload,
}

/// How a turn that watches an actor saw it: live as the turn's step began, or stood up earlier in the step, or
/// already ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seen {
    /// Live: its ending is still to come.
    Live,
    /// Ended: its closing turn runs in this step, or has run in one before.
    Ended,
}

/// How much a logged line matters (ADR-0009).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

/// The code an actor is made from (ADR-0009).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    /// The program's own actor type, as it declares itself (ADR-0013).
    Linked(&'static Declared),
    /// An actor type a blob makes, as its section declares it, kept by the blob's host (ADR-0015).
    Blob(&'static BlobType),
}

/// An actor type a blob makes: the blob, and what the type declares, as the blob's section says. Its host keeps one
/// for each type of each blob it takes in, so a [`Code`] names it by one pointer, as it names a linked type.
#[derive(Debug, PartialEq, Eq)]
pub struct BlobType {
    pub blob: BlobId,
    pub declared: &'static Declared,
}

impl BlobType {
    /// Whether it is the type of the blob `blob` that declares itself as `declared` does, by its kind.
    #[must_use]
    pub fn is(&self, blob: BlobId, declared: &Declared) -> bool {
        let is_blob = self.blob == blob;
        let is_kind = self.declared.id() == declared.id();

        is_blob && is_kind
    }
}

impl Code {
    /// What the actor type the code makes declares (ADR-0013).
    #[must_use]
    pub const fn declared(&self) -> &'static Declared {
        match self {
            Self::Linked(declared) => declared,
            Self::Blob(made) => made.declared,
        }
    }

    /// The kind of actor the code makes: the actor's name in its path.
    #[must_use]
    pub const fn kind(&self) -> &KindName {
        match self {
            Self::Linked(declared) => declared.name(),
            Self::Blob(made) => made.declared.name(),
        }
    }

    /// The segment of the singleton this code makes.
    #[must_use]
    pub fn singleton(&self) -> PathSegment {
        PathSegment::new(self.kind().clone(), &())
    }

    /// The segment of the actor this code makes under the key named `key`.
    #[must_use]
    pub fn instanced(&self, key: &KeyName) -> PathSegment {
        PathSegment::instanced(self.kind().clone(), key.clone())
    }
}
