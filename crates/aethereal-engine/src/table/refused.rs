//! Why the table turned a change down (ADR-0008).

use aethereal_data::ActorPath;
use std::error::Error;
use std::fmt;

/// Why the table did not take a new actor (ADR-0008).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The kind and key have been used beneath that parent, by an actor that is live or one that has closed.
    KeyTaken,
    /// The child would stand deeper than [`ActorPath::MAX_DEPTH`] segments.
    TooDeep,
    /// The parent named is not in the table, or has closed.
    ParentNotLive,
    /// Another path in the table already has the id this path folds to.
    IdShared,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeyTaken => write!(f, "actor table: the kind and key have been used beneath that parent"),
            Self::TooDeep => {
                write!(f, "actor table: a child would stand deeper than {} segments", ActorPath::MAX_DEPTH)
            }
            Self::ParentNotLive => write!(f, "actor table: the parent is not a live actor"),
            Self::IdShared => write!(f, "actor table: another path already has this path's id"),
        }
    }
}

impl Error for Refused {}

/// An actor was to be closed that is not in the table, or has closed already (ADR-0008).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotLive;

impl fmt::Display for NotLive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "actor table: the actor to close is not live")
    }
}

impl Error for NotLive {}
