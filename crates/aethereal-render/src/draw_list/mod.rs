//! Draw lists: an actor's own placements in a pass, in the order it lists them, kept until it sends another (ADR-0031).
//! Each list is checked whole as it is handled, and what it found is kept, so drawing it looks nothing up and refuses
//! nothing.

#[cfg(test)]
mod tests;

use crate::frame::Frame;
use crate::pass::{Pass, PassName};
use crate::placement::PlacementId;
use aethereal_data::{ActorId, Plain};
use std::collections::HashMap;

/// The placements its sender draws in the pass `pass` of `frame`, in order, each joined to it
/// [`Listed`](crate::placement::Order::Listed), in place of the sender's last list for that pass from the next
/// `Render`. It has no answer: a list refused is logged, and the last stands.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.draw_list")]
pub struct DrawList {
    pub frame: Frame,
    pub pass: PassName,
    pub placements: Vec<PlacementId>,
}

impl DrawList {
    /// The placements to draw in the pass `P` of `frame`, in order: the pass named by its type.
    #[must_use]
    pub const fn of<P: Pass>(frame: Frame, placements: Vec<PlacementId>) -> Self {
        Self { frame, pass: PassName::of::<P>(), placements }
    }
}

/// How a placement's join of one pass is drawn: in every frame, or while its maker's list holds it, stamped with the
/// number of the last list checked that named it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Listing {
    Loose,
    /// `stamp` is zero until a list names it, as lists are numbered from one.
    Listed {
        stamp: u64,
    },
}

/// Why a list was refused whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListRefused {
    /// No frame is kept as the one named: a frame that ended on a texture and was released, or a window not drawn to.
    Frame,
    /// The id at `at`, counted from zero, was refused.
    Entry { at: u32, why: EntryRefused },
}

/// Why one id of a list was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryRefused {
    /// No placement is kept under the id: never made, or released.
    Placement,
    /// Another actor made the placement, and only its maker lists it.
    NotOwner,
    /// The placement does not join the pass of the frame named.
    Unjoined,
    /// The placement joins the pass loose: it is drawn in every frame, and no list orders it.
    Loose,
    /// An earlier id of the list names the same placement.
    Twice,
}

/// One placement of a kept list, as its check found it: the placement, and which of its joins draws in the list's
/// pass, counted from zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Entry {
    pub placement: u64,
    pub join: usize,
}

/// What is kept, as a list's check looks it up.
pub(crate) trait Placements {
    /// The join of the placement `placement` that draws in `pass`, a pass of a frame, counted from zero, with how it
    /// is drawn, to stamp: refused when it is not kept, `by` did not make it, or it joins no such pass.
    fn listing(
        &mut self,
        placement: u64,
        by: ActorId,
        pass: (Frame, PassName),
    ) -> Result<(usize, &mut Listing), EntryRefused>;
}

/// The lists the renderer keeps: one for each actor in each pass of each frame, its last that was let through, with
/// the number of the last list checked, which each list's check stamps the joins it names with.
#[derive(Debug, Default)]
pub(crate) struct Lists {
    kept: HashMap<(Frame, PassName), HashMap<ActorId, Vec<Entry>>>,
    checked: u64,
}

impl Lists {
    /// Checks `list` whole, sent by `by`, against the placements `placements` keeps, and keeps it in place of `by`'s
    /// last list for its pass: each id names a placement `by` made, kept, that joins the pass `Listed`, and none is
    /// named twice. One that fails is refused, and `by`'s last list for the pass stands.
    pub fn check(&mut self, list: &DrawList, by: ActorId, placements: &mut impl Placements) -> Result<(), ListRefused> {
        self.checked += 1;
        let number = self.checked;
        let pass = (list.frame, list.pass);
        let mut entries = Vec::with_capacity(list.placements.len());
        for (placement, at) in list.placements.iter().zip(0..) {
            let refused = |why| ListRefused::Entry { at, why };
            let (join, listing) = placements.listing(placement.id, by, pass).map_err(refused)?;
            match listing {
                Listing::Loose => return Err(refused(EntryRefused::Loose)),
                Listing::Listed { stamp } if *stamp == number => return Err(refused(EntryRefused::Twice)),
                Listing::Listed { stamp } => *stamp = number,
            }
            entries.push(Entry { placement: placement.id, join });
        }
        self.kept.entry(pass).or_default().insert(by, entries);

        Ok(())
    }

    /// Takes the placement `placement`, which its maker `by` released, out of each of `by`'s lists for the passes
    /// `passes` it joined listed, keeping the rest in order.
    pub fn released(&mut self, placement: u64, by: ActorId, passes: impl IntoIterator<Item = (Frame, PassName)>) {
        for pass in passes {
            let list = self.kept.get_mut(&pass).and_then(|lists| lists.get_mut(&by));
            if let Some(list) = list {
                list.retain(|entry| entry.placement != placement);
            }
        }
    }

    /// Drops every list of `actor`, which ended.
    pub fn ended(&mut self, actor: ActorId) {
        for lists in self.kept.values_mut() {
            lists.remove(&actor);
        }
        self.kept.retain(|_, lists| !lists.is_empty());
    }

    /// Drops every list for `frame`, which is gone: a frame that ended on a texture, released, or a closed window's
    /// main frame.
    pub fn unframed(&mut self, frame: Frame) {
        self.kept.retain(|(listed, _), _| *listed != frame);
    }

    /// Each actor's list for the pass `pass` of `frame`, in no order among actors.
    pub fn of(&self, frame: Frame, pass: PassName) -> impl Iterator<Item = &[Entry]> {
        self.kept.get(&(frame, pass)).into_iter().flat_map(HashMap::values).map(Vec::as_slice)
    }
}
