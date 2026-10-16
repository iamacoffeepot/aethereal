//! The children a seated actor has made, kept in its seat (ADR-0008).

use crate::by_actor::IdBits;
use aethereal_actor::Children;
use aethereal_data::ActorId;
use std::collections::HashSet;
use std::hash::BuildHasherDefault;

/// How many children's ids are kept in the seat itself, before they move to a set of their own.
const FEW: usize = 3;

/// The ids of the children an actor has made, live or ended, whose keys it may not use again.
///
/// A parent's own knowledge, so it is kept with the parent where it is
/// seated, and goes when the parent's seat does: nothing is spawned beneath an
/// actor that has ended. Most actors make a few children or none, so the
/// first few are kept in the seat with nothing allocated.
pub(super) enum Made {
    /// At most [`FEW`]: the first `count` of `ids`.
    Few { ids: [u64; FEW], count: usize },
    /// More than [`FEW`].
    Many(HashSet<u64, BuildHasherDefault<IdBits>>),
}

impl Made {
    /// An actor that has made no child.
    pub(super) const fn none() -> Self {
        Self::Few { ids: [0; FEW], count: 0 }
    }

    /// Notes that the actor made the child `child`.
    pub(super) fn insert(&mut self, child: ActorId) {
        match self {
            Self::Few { ids, count } if *count < FEW => {
                ids[*count] = child.bits();
                *count += 1;
            }
            Self::Few { ids, .. } => {
                let mut many: HashSet<u64, BuildHasherDefault<IdBits>> = ids.iter().copied().collect();
                many.insert(child.bits());
                *self = Self::Many(many);
            }
            Self::Many(many) => {
                many.insert(child.bits());
            }
        }
    }
}

impl Children for Made {
    fn has_made(&self, child: ActorId) -> bool {
        match self {
            Self::Few { ids, count } => ids[..*count].contains(&child.bits()),
            Self::Many(many) => many.contains(&child.bits()),
        }
    }
}
