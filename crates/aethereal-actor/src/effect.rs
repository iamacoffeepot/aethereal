//! An effect: one thing an actor asks of the engine (ADR-0009).

use crate::{ErasedActorRef, Fault, How, MailId, Payload};
use aethereal_data::{ActorId, KeyName, KindId, KindName, PathSegment};
use alloc::string::String;

/// One thing an actor asks of the engine, and which actor asked (ADR-0009).
///
/// A turn returns a list of these, and nothing a turn asks for happens during
/// the turn. A view is folded from them (ADR-0011).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effect {
    by: ActorId,
    what: What,
}

impl Effect {
    /// The effect by which the actor `by` asks for `what`.
    #[must_use]
    pub const fn new(by: ActorId, what: What) -> Self {
        Self { by, what }
    }

    /// The id of the actor that asked.
    #[must_use]
    pub const fn by(&self) -> ActorId {
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
    /// Mail of kind `kind`, whose bytes are at `bytes`, in the arena of the worker that ran the turn or in bytes of
    /// their own, is sent to the actor `to`, as the turn resolved the target it was given. `id` is the mail's own:
    /// the asking actor, and its count of mail sent.
    Send { to: ActorId, id: MailId, kind: KindId, bytes: Payload },
    /// A root, a singleton, has been made. Written by the engine for the chassis, by the root itself.
    Root { from: Code },
    /// A root of an instanced type has been made. Written by the engine for the chassis, by the root itself.
    RootInstanced { from: Code, key: KeyName },
    /// A child, a singleton, stands beneath the asking actor and takes its own mail.
    Spawn { from: Code },
    /// A child of an instanced type stands beneath the asking actor and takes its own mail.
    SpawnInstanced { from: Code, key: KeyName },
    /// A child, a singleton, stands beneath the asking actor, and its mail is handed to that actor.
    Embed { from: Code },
    /// A child of an instanced type stands beneath the asking actor, and its mail is handed to that actor.
    EmbedInstanced { from: Code, key: KeyName },
    /// The asking actor's `begin` failed, and the actor was never live. It reports, and asks for nothing.
    Faulted,
    /// The asking actor's turn failed with this fault, and the actor has ended. Written by the engine, since the
    /// actor asked for nothing.
    Failed(Fault),
    /// The asking actor's closing turn failed with this fault. Written by the engine. The actor had already
    /// ended, so this marks nothing: it is there for the log.
    EndFailed(Fault),
    /// The asking actor's closing turn has run, and its life is over: this is how it ended. Written by the engine,
    /// with the closing turn's effects, so it comes after the actor's last mail.
    Ended(How),
    /// The asking actor is shown the ending of `of`, once, when that actor ends.
    Watch { of: ErasedActorRef },
    /// The asking actor closes.
    Close,
    /// The asking actor logs `text`. It is kept when the turn that asked for it fails.
    Log { level: Level, text: String },
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
///
/// A guest's code, a blob the engine holds, is a further case that enters with blobs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Code {
    /// The program's own actor of this kind.
    Linked(KindName),
}

impl Code {
    /// The kind of actor the code makes: the actor's name in its path.
    #[must_use]
    pub const fn kind(&self) -> &KindName {
        match self {
            Self::Linked(kind) => kind,
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
