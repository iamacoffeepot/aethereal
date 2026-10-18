//! Why a spawn gave no child (ADR-0009).

use crate::Fault;
use aethereal_data::ActorPath;
use core::error::Error;
use core::fmt;

/// Why a spawn gave no child (ADR-0009), or a put no blob (ADR-0014). The actor handles it in the handler that asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// The kind and key have been used beneath this actor, by a child that is live or one that has ended.
    KeyTaken,
    /// The child would stand deeper than [`ActorPath::MAX_DEPTH`] segments.
    TooDeep,
    /// The child's `begin` returned this fault. The child was never live, and its key is still free.
    Began(Fault),
    /// The blob's bytes would take the turn's worker past its share of the budget for blobs in this step.
    NoRoom,
    /// The blob is not code the engine holds, or makes no actor of the child's type (ADR-0009).
    NotMade,
    /// The child would begin in an instance that is waiting on a call out of its own: a chain of `begin`s left its
    /// blob and came back to it within one turn (ADR-0015).
    Waiting,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeyTaken => write!(f, "spawn: the kind and key have been used beneath this actor"),
            Self::TooDeep => write!(f, "spawn: a child would stand deeper than {} segments", ActorPath::MAX_DEPTH),
            Self::Began(_) => write!(f, "spawn: the child's begin returned a fault"),
            Self::NoRoom => write!(f, "put: the blob would take its worker past its share of the budget for blobs"),
            Self::NotMade => write!(f, "spawn: the blob makes no actor of the child's type"),
            Self::Waiting => write!(f, "spawn: the child's instance is waiting on a call out of its own"),
        }
    }
}

impl Error for Refused {}
