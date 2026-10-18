//! Where a batch's items come from: an actor's waiting items, or one hop (ADR-0011, ADR-0013, ADR-0015).

use super::Lists;
use super::Reading;
use super::fuse::Hop;
use crate::stream::{self, Handed, Pieces};
use crate::{Context, Envelope};
use aethereal_actor::{Chain, Item, Items, Mail, Peeked};

/// An actor's waiting items, read from `pieces`.
pub(super) struct Waiting<'w, 'r, 'p> {
    pub(super) reading: &'w Reading<'r>,
    pub(super) pieces: &'w mut Pieces<'p>,
    /// The bytes of the mail its part carried over.
    pub(super) carried: &'p [u8],
}

/// The item `handed` is, as a turn reads it, and where it stands in its chain: bytes read from `reading`'s arenas, or
/// from `carried`.
#[allow(clippy::inline_always, reason = "once for every item, read inline in the loop that hands it")]
#[inline(always)]
fn read<'a>(
    handed: Handed<'a>,
    reading: &Reading<'_>,
    mail: &'a [aethereal_actor::Arena],
    carried: &'a [u8],
) -> Peeked<'a> {
    let source = handed.source();
    let (item, context) = match handed {
        Handed::Mail { envelope: &Envelope { kind, root, depth, .. }, bytes, .. } => {
            let bytes = stream::read(bytes, mail, carried);
            let mail = Mail::new(kind, bytes, source, root, depth, reading.read_limit);

            (Item::Mail(mail), Context { root, depth: depth + 1 })
        }
        Handed::Ended { ended, context } => (Item::Ended(ended.clone()), *context),
    };
    // The item sits one above what its turn sends in its chain.
    let chain = Chain { root: context.root, depth: context.depth - 1 };

    Peeked { item, source, chain }
}

impl<'r: 'p, 'p> Items<'p> for Waiting<'_, 'r, 'p> {
    #[inline]
    fn next(&mut self) -> Option<Peeked<'p>> {
        let handed = self.pieces.next()?;

        Some(read(handed, self.reading, self.reading.mail, self.carried))
    }

    fn each(&self, count: usize, visit: &mut dyn FnMut(Peeked<'p>)) {
        for handed in self.pieces.clone().take(count) {
            visit(read(handed, self.reading, self.reading.mail, self.carried));
        }
    }
}

/// The item `hop` is, a piece of mail handed straight to its receiver in the step (ADR-0013), its bytes kept in
/// `lists`.
pub(super) fn hop_item<'w>(reading: &Reading<'_>, hop: &Hop, lists: &'w Lists) -> Peeked<'w> {
    let mail = Mail::new(hop.kind, lists.fusing.bytes(hop), hop.from, hop.root, hop.depth, reading.read_limit);

    Peeked { item: Item::Mail(mail), source: hop.from, chain: Chain { root: hop.root, depth: hop.depth } }
}
