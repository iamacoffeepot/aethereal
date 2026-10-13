//! An effect: one thing an actor asks of the engine (ADR-0009).

use aethereal_data::{ActorId, KeyName, KindName, PathSegment};

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
}

/// What an effect asks for (ADR-0009).
///
/// The cases here are the ones the table of actors is folded from. The others
/// ADR-0009 lists enter with the piece of the engine that applies each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum What {
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
    /// The asking actor's turn failed, and the actor has ended. Written by the engine, since the actor asked for
    /// nothing.
    Failed,
    /// The asking actor closes.
    Close,
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
