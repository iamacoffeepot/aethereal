//! A short reference to an actor's path (ADR-0008).

use crate::Plain;

/// A short reference to an actor's path: 64 bits that stand for it within one engine (ADR-0008).
///
/// Only the engine computes one, and the function that does enters with the
/// engine. An id is not an address: nothing sends to one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Plain)]
pub struct ActorId(u64);
