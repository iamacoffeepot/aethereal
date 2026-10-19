//! A resource through which the host brings mail into the engine (ADR-0010).

use crate::Resource;
use aethereal_data::Kind;
use core::marker::PhantomData;

/// A resource through which the host brings mail of kind `K` to the actor that owns it (ADR-0010).
///
/// It holds nothing to open. What the host brings through it arrives at its
/// owner as mail of kind `K` from that owner, in the step after the host
/// brings it. The chassis makes one with the engine, beside the feed it pushes
/// onto, and hands it to a root in its config. From then on it is given and
/// taken as any resource is, and the host's mail goes to whoever owns it as it
/// is brought. Once it is dropped, with its owner or with mail that did not
/// arrive, its feed is closed.
pub struct Source<K> {
    of: PhantomData<fn() -> K>,
}

impl<K: Kind + 'static> Resource for Source<K> {}

/// A source with no feed brings nothing: the engine makes the one a feed is for.
impl<K> Default for Source<K> {
    fn default() -> Self {
        Self { of: PhantomData }
    }
}
