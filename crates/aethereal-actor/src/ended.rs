//! An ending an actor is shown: a watched actor that has ended, and how (ADR-0009).

use crate::{ErasedActorRef, Fault, Seated};

/// An actor this one was watching, which has ended (ADR-0009).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ended {
    actor: Seated,
    how: How,
}

impl Ended {
    /// The ending of the actor `actor` names.
    ///
    /// For the engine, which has established that the actor was made.
    #[must_use]
    pub const fn new(actor: Seated, how: How) -> Self {
        Self { actor, how }
    }

    /// The actor that ended.
    #[must_use]
    pub const fn actor(&self) -> ErasedActorRef {
        ErasedActorRef::new(self.actor)
    }

    /// How it ended.
    #[must_use]
    pub const fn how(&self) -> &How {
        &self.how
    }
}

/// How a watched actor ended (ADR-0009).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum How {
    /// It asked to close.
    Closed,
    /// A turn of its own returned this fault.
    Failed(Fault),
    /// It had ended in a step before the one the watch was asked for in. How is not kept.
    Earlier,
}
