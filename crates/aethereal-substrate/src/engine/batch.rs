//! The batches the engine prepares an actor for before their turns: its waiting items, as many as it takes turns of
//! between two looks at the clock, or one hop (ADR-0011, ADR-0013, ADR-0015).

use super::Lists;
use super::Reading;
use super::ask::{self, Answering};
use super::fuse::Hop;
use crate::stream::{self, Handed, Pieces};
use crate::{Context, Envelope};
use aethereal_actor::{Arena, Batch, Chain, Item, Mail, Peeked, Pinned, Pinning};
use std::iter;

/// A batch of one actor's waiting items: `first`, and after it as many of `rest` as make
/// [`SLICE_EVERY`](super::SLICE_EVERY), or as are left.
pub(super) struct Waiting<'w, 'r, 'p> {
    pub(super) reading: &'w Reading<'r>,
    pub(super) first: Handed<'p>,
    pub(super) rest: &'w Pieces<'p>,
    /// The bytes of the mail its part carried over, and the pins of the blobs they refer to.
    pub(super) carried: &'p [u8],
    pub(super) carried_pins: &'p [Pinning],
}

/// A batch of one hop, a piece of mail handed straight to its receiver in the step (ADR-0013).
pub(super) struct OneHop<'w, 'r> {
    pub(super) reading: &'w Reading<'r>,
    pub(super) hop: &'w Hop,
    pub(super) lists: &'w Lists,
    /// The arena the hop's bytes were written in, which holds the pins of the blobs they refer to.
    pub(super) arena: &'w Arena,
}

/// The item `handed` is, as a turn reads it, handed to an actor that is `answering` it, and the context of what its
/// turn asks for: bytes read from `reading`'s arenas, or from `carried`.
#[allow(clippy::inline_always, reason = "once for every item, read inline in the loop that hands it")]
#[inline(always)]
pub(super) fn read<'a>(
    handed: Handed<'a>,
    reading: &Reading<'a>,
    carried: &'a [u8],
    answering: Answering<'a>,
) -> (Item<'a>, Context) {
    let source = handed.source();
    match handed {
        Handed::Mail { envelope: &Envelope { kind, root, depth, .. }, bytes, .. } => {
            let bytes = stream::read(bytes, reading.mail, carried);
            let mail = Mail::new(kind, bytes, source, root, depth, reading.read_limit);

            (ask::item(mail, answering), Context { root, depth: depth + 1 })
        }
        Handed::Ended { ended, context } => (Item::Ended(ended.clone()), *context),
    }
}

impl Batch for Waiting<'_, '_, '_> {
    fn each(&self, visit: &mut dyn FnMut(Peeked<'_>)) {
        let rest = self.rest.clone().take(super::SLICE_EVERY as usize - 1);
        for handed in iter::once(self.first).chain(rest) {
            let source = handed.source();
            let pinned = match handed {
                Handed::Mail { bytes, .. } => stream::pinned(bytes, self.reading.mail, self.carried_pins),
                Handed::Ended { .. } => Pinned::Nothing,
            };
            let (item, context) = read(handed, self.reading, self.carried, Answering::Not);
            let chain = Chain { root: context.root, depth: context.depth - 1 };
            visit(Peeked { item, source, chain, pinned });
        }
    }
}

impl Batch for OneHop<'_, '_> {
    fn each(&self, visit: &mut dyn FnMut(Peeked<'_>)) {
        let hop = self.hop;
        let bytes = self.lists.fusing.bytes(hop);
        let mail = Mail::new(hop.kind, bytes, hop.from, hop.root, hop.depth, self.reading.read_limit);
        let chain = Chain { root: hop.root, depth: hop.depth };
        let pinned = self.arena.pinned(hop.sent);
        visit(Peeked { item: Item::Mail(mail), source: hop.from, chain, pinned });
    }
}
