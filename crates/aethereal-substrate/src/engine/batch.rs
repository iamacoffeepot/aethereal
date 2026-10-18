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

/// One hop, a piece of mail handed straight to its receiver in the step (ADR-0013), until it is taken.
pub(super) struct OneHop<'w> {
    hop: &'w Hop,
    bytes: &'w [u8],
    read_limit: usize,
    taken: bool,
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

impl<'w> OneHop<'w> {
    /// The hop `hop`, whose bytes `lists` keeps, as `reading` reads mail.
    pub(super) fn new(reading: &Reading<'_>, hop: &'w Hop, lists: &'w Lists) -> Self {
        Self { hop, bytes: lists.fusing.bytes(hop), read_limit: reading.read_limit, taken: false }
    }

    fn peeked(&self) -> Peeked<'w> {
        let hop = self.hop;
        let mail = Mail::new(hop.kind, self.bytes, hop.from, hop.root, hop.depth, self.read_limit);

        Peeked { item: Item::Mail(mail), source: hop.from, chain: Chain { root: hop.root, depth: hop.depth } }
    }
}

impl<'w> Items<'w> for OneHop<'w> {
    fn next(&mut self) -> Option<Peeked<'w>> {
        if self.taken {
            return None;
        }
        self.taken = true;

        Some(self.peeked())
    }

    fn each(&self, count: usize, visit: &mut dyn FnMut(Peeked<'w>)) {
        if !self.taken && count > 0 {
            visit(self.peeked());
        }
    }
}
