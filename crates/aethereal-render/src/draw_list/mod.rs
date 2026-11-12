//! Draw lists: what an actor draws, in the order it lists it, kept until it sends another (ADR-0031). A list holds its
//! sender's own placements and calls of its direct children, and the renderer walks the lists from the program's root
//! list, entering each child where it is called, so the order of every draw is declared (ADR-0055). Each list is
//! checked whole as it is handled, and what it found is kept, so drawing it looks nothing up and refuses nothing.

#[cfg(test)]
mod tests;

use crate::placement::PlacementId;
use aethereal_actor::records::Naming;
use aethereal_actor::{Child, ChildOf, ParentOf};
use aethereal_data::{ActorId, ActorPath, PathSegment, Plain};
use std::collections::HashMap;
use std::{slice, vec};

/// What its sender draws, in order: its own placements, and the direct children it calls, each drawn where it is
/// called, in place of the sender's last list from the next `Render`. A placement draws in each pass one of its listed
/// joins names, so one list orders the sender's drawing in every pass of every frame (ADR-0055). It has no answer: a
/// list refused is logged, and the last stands.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.draw_list")]
pub struct DrawList {
    pub entries: Vec<Entry>,
}

/// One entry of a draw list.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.entry")]
pub enum Entry {
    /// A placement of the sender's, of a mesh or a text, drawn in each pass it joins
    /// [`Listed`](crate::placement::Order::Listed).
    Draw { placement: PlacementId },
    /// What a direct child of the sender draws, drawn here.
    Call(Callee),
}

/// A direct child of a list's sender, by its segment beneath the sender: what [`Entry::call`] names.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.callee")]
pub struct Callee {
    child: Naming,
}

impl Entry {
    /// A call of `child`, a child of the list's sender, which `sender` refers to, as `turn.me()` answers: a place
    /// deeper than a child, or one whose type does not stand beneath the sender's, does not build. The renderer finds
    /// the child one segment beneath whoever sends the list, so a call reaches the sender's own child and nothing else.
    #[must_use]
    pub fn call<C: ChildOf>(_sender: impl ParentOf<C>, child: &Child<C>) -> Self {
        Self::Call(Callee { child: Naming::of(child.segment()) })
    }

    /// What the renderer keeps of it: a placement by its id, or a call by its child's, which `child_id` answers for the
    /// child's segment beneath the list's sender.
    pub(crate) fn listed(&self, child_id: impl Fn(&PathSegment) -> ActorId) -> Listed {
        match self {
            Self::Draw { placement } => Listed::Draw(placement.id),
            Self::Call(callee) => Listed::Call(child_id(&callee.child.segment())),
        }
    }
}

/// One entry of a list the renderer keeps: a placement of its sender's, by its id, or a child its sender calls, by
/// the child's id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Listed {
    Draw(u64),
    Call(ActorId),
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

/// Why a list was refused whole: the entry at `at`, counted from zero, was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ListRefused {
    pub at: u32,
    pub why: EntryRefused,
}

/// Why one entry of a list was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryRefused {
    /// No placement is kept under the id: never made, or released.
    Placement,
    /// Another actor made the placement, and only its maker lists it.
    NotOwner,
    /// The placement joins every pass it draws in loose: it is drawn in every frame, and no list orders it.
    Loose,
    /// An earlier entry of the list names the same placement, or calls the same child.
    Twice,
}

/// What is kept, as a list's check looks it up.
pub(crate) trait Placements {
    /// Stamps the placement `placement`'s joins with `number`, the number of the list being checked, as [`stamp`]
    /// does: refused when it is not kept, or `by` did not make it.
    fn stamp(&mut self, placement: u64, by: ActorId, number: u64) -> Result<(), EntryRefused>;
}

/// Stamps each of a placement's `listings` that is listed with `number`, the number of the list being checked: refused
/// when none is, as no list orders a placement joined only loose, or when one holds `number` already, as the list
/// named the placement before.
pub(crate) fn stamp<'a>(listings: impl IntoIterator<Item = &'a mut Listing>, number: u64) -> Result<(), EntryRefused> {
    let mut listed = false;
    for listing in listings {
        match listing {
            Listing::Loose => {}
            Listing::Listed { stamp } if *stamp == number => return Err(EntryRefused::Twice),
            Listing::Listed { stamp } => {
                *stamp = number;
                listed = true;
            }
        }
    }

    if listed {
        Ok(())
    } else {
        Err(EntryRefused::Loose)
    }
}

/// The lists the renderer keeps: one for each actor, its last that was let through, with the program's root list, the
/// roots a window's main frame is walked from, and the number of the last list checked, which each list's check
/// stamps the joins it names with.
#[derive(Debug)]
pub(crate) struct Lists {
    roots: Vec<ActorId>,
    kept: HashMap<ActorId, Vec<Listed>>,
    checked: u64,
}

impl Lists {
    /// No list kept, with the program's root list `roots`, fixed for the program's life (ADR-0055).
    pub fn new(roots: Vec<ActorId>) -> Self {
        Self { roots, kept: HashMap::new(), checked: 0 }
    }

    /// The program's root list: the roots a window's main frame is walked from, in draw order.
    pub fn roots(&self) -> &[ActorId] {
        &self.roots
    }

    /// Checks `list` whole, sent by `by`, against the placements `placements` keeps, and keeps it in place of `by`'s
    /// last list: each placement is one `by` made, kept, that joins a pass `Listed`, and no placement or child is named
    /// twice. One that fails is refused, and `by`'s last list stands.
    pub fn check(
        &mut self,
        list: Vec<Listed>,
        by: ActorId,
        placements: &mut impl Placements,
    ) -> Result<(), ListRefused> {
        self.checked += 1;
        let number = self.checked;
        for (entry, at) in list.iter().zip(0..) {
            let refused = |why| ListRefused { at, why };
            match *entry {
                Listed::Draw(placement) => placements.stamp(placement, by, number).map_err(refused)?,
                Listed::Call(_) => {
                    let twice = list.iter().take(at as usize).any(|earlier| earlier == entry);
                    if twice {
                        return Err(refused(EntryRefused::Twice));
                    }
                }
            }
        }
        self.kept.insert(by, list);

        Ok(())
    }

    /// Takes the placement `placement`, which its maker `by` released, out of `by`'s list, keeping the rest in order.
    pub fn released(&mut self, placement: u64, by: ActorId) {
        if let Some(list) = self.kept.get_mut(&by) {
            list.retain(|entry| *entry != Listed::Draw(placement));
        }
    }

    /// Drops the list of `actor`, which ended. A list that calls it keeps the call: an actor made again where it stood
    /// is drawn there once it sends a list.
    pub fn ended(&mut self, actor: ActorId) {
        self.kept.remove(&actor);
    }

    /// Every placement every list kept draws, in no order.
    pub fn all(&self) -> impl Iterator<Item = u64> {
        self.kept.values().flatten().filter_map(|entry| match entry {
            Listed::Draw(placement) => Some(*placement),
            Listed::Call(_) => None,
        })
    }

    /// The placements drawn from `roots`, in the order of the walk: each root's list in turn, each entered where it is
    /// called, so a list's calls draw between its own placements as it lists them (ADR-0055). An actor no list from
    /// `roots` calls is not drawn.
    pub fn walk(&self, roots: Vec<ActorId>) -> Walked<'_> {
        Walked { kept: &self.kept, roots: roots.into_iter(), entered: Vec::new() }
    }
}

/// The walk of the lists from a set of roots, as [`Lists::walk`] answers it.
pub(crate) struct Walked<'a> {
    kept: &'a HashMap<ActorId, Vec<Listed>>,
    roots: vec::IntoIter<ActorId>,
    /// The lists being drawn, the one called last on top: a stack, so the walk's depth costs no frames.
    entered: Vec<slice::Iter<'a, Listed>>,
}

impl Walked<'_> {
    /// Enters the list of `actor`, when it has one. No actor stands deeper than a path's most segments, so a call that
    /// would go deeper, which only two ids that collide could write, is not entered.
    fn enter(&mut self, actor: ActorId) {
        let Some(list) = self.kept.get(&actor) else {
            return;
        };
        let within = self.entered.len() < ActorPath::MAX_DEPTH;
        if within {
            self.entered.push(list.iter());
        }
    }
}

impl Iterator for Walked<'_> {
    type Item = u64;

    fn next(&mut self) -> Option<u64> {
        loop {
            let Some(list) = self.entered.last_mut() else {
                let root = self.roots.next()?;
                self.enter(root);
                continue;
            };
            match list.next() {
                Some(Listed::Draw(placement)) => return Some(*placement),
                Some(Listed::Call(child)) => self.enter(*child),
                None => {
                    self.entered.pop();
                }
            }
        }
    }
}
