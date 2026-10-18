//! What an actor asks of the engine, in three lists a turn fills as it runs: the mail it sends, the lines it logs,
//! and the rest, its effects on the lives of actors (ADR-0009).

use crate::{BlobId, Declared, ErasedActorRef, Fault, How, Payload, SeatNumber, Seated};
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
}

/// A piece of mail a turn sends, or a run of them (ADR-0009, ADR-0011).
///
/// Mail of kind `kind`, whose bytes are at `bytes`, in the arena of the worker that ran the turn or in bytes of
/// their own, is sent by the actor `by` names to the actor `to` names, in its seat, as the turn resolved the target
/// it was given. `count` is the sender's count of mail sent, which with the sender is the mail's id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mailed {
    pub by: Seated,
    pub to: Seated,
    pub count: u64,
    pub kind: KindId,
    pub bytes: Payload,
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

/// Who a turn may send to: a closed set (ADR-0009).
///
/// Every case is an actor beneath the sender, which owns that part of the
/// tree, or one the engine has already established exists. There is no send
/// to a bare path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum To {
    /// The actor directly above the sender.
    Parent,
    /// The singleton of this kind directly beneath the sender.
    Child(KindName),
    /// The actor of this kind and key directly beneath the sender.
    ChildInstanced(KindName, KeyName),
    /// The source of the item being handled: the sender of mail, or the actor that ended.
    Sender,
    /// The actor a reference the sender holds refers to.
    Held(ErasedActorRef),
}

/// Who a `begin` or a closing turn may send to (ADR-0009).
///
/// Neither is handling a piece of mail, so neither has a sender: this is
/// [`To`] without that case, and a send to the sender from either does not
/// build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToWithoutMail {
    /// The actor directly above the sender.
    Parent,
    /// The singleton of this kind directly beneath the sender.
    Child(KindName),
    /// The actor of this kind and key directly beneath the sender.
    ChildInstanced(KindName, KeyName),
    /// The actor a reference the sender holds refers to.
    Held(ErasedActorRef),
}

impl From<ToWithoutMail> for To {
    fn from(to: ToWithoutMail) -> Self {
        match to {
            ToWithoutMail::Parent => Self::Parent,
            ToWithoutMail::Child(kind) => Self::Child(kind),
            ToWithoutMail::ChildInstanced(kind, key) => Self::ChildInstanced(kind, key),
            ToWithoutMail::Held(reference) => Self::Held(reference),
        }
    }
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
