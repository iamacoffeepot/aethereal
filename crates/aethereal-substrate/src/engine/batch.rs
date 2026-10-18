//! The batches the engine shows an actor before their turns: its waiting items, as many as the rest of its slice is
//! expected to cover, or one hop (ADR-0011, ADR-0013, ADR-0015).

use super::Lists;
use super::Reading;
use super::fuse::Hop;
use crate::stream::{self, Handed, Pieces};
use crate::{Context, Envelope};
use aethereal_actor::{Batch, Chain, Item, Mail, Peeked};

/// A batch of one actor's waiting items: the next `fits` of `pieces`.
pub(super) struct Waiting<'w, 'r, 'p> {
    pub(super) reading: &'w Reading<'r>,
    pub(super) pieces: &'w Pieces<'p>,
    /// The bytes of the mail its part carried over.
    pub(super) carried: &'p [u8],
    pub(super) fits: usize,
}

/// A batch of one hop, a piece of mail handed straight to its receiver in the step (ADR-0013).
pub(super) struct OneHop<'w, 'r> {
    pub(super) reading: &'w Reading<'r>,
    pub(super) hop: &'w Hop,
    pub(super) lists: &'w Lists,
}

/// The item `handed` is, as a turn reads it, and the context of what its turn asks for: bytes read from `reading`'s
/// arenas, or from `carried`.
#[allow(clippy::inline_always, reason = "once for every item, read inline in the loop that hands it")]
#[inline(always)]
pub(super) fn read<'a>(handed: Handed<'a>, reading: &Reading<'a>, carried: &'a [u8]) -> (Item<'a>, Context) {
    let source = handed.source();
    match handed {
        Handed::Mail { envelope: &Envelope { kind, root, depth, .. }, bytes, .. } => {
            let bytes = stream::read(bytes, reading.mail, carried);
            let mail = Mail::new(kind, bytes, source, root, depth, reading.read_limit);

            (Item::Mail(mail), Context { root, depth: depth + 1 })
        }
        Handed::Ended { ended, context } => (Item::Ended(ended.clone()), *context),
    }
}

impl Batch for Waiting<'_, '_, '_> {
    fn fits(&self) -> usize {
        self.fits
    }

    fn each(&self, visit: &mut dyn FnMut(Peeked<'_>)) {
        for handed in self.pieces.clone().take(self.fits) {
            let source = handed.source();
            let (item, context) = read(handed, self.reading, self.carried);
            let chain = Chain { root: context.root, depth: context.depth - 1 };
            visit(Peeked { item, source, chain });
        }
    }
}

impl Batch for OneHop<'_, '_> {
    fn fits(&self) -> usize {
        1
    }

    fn each(&self, visit: &mut dyn FnMut(Peeked<'_>)) {
        let hop = self.hop;
        let bytes = self.lists.fusing.bytes(hop);
        let mail = Mail::new(hop.kind, bytes, hop.from, hop.root, hop.depth, self.reading.read_limit);
        let chain = Chain { root: hop.root, depth: hop.depth };
        visit(Peeked { item: Item::Mail(mail), source: hop.from, chain });
    }
}
