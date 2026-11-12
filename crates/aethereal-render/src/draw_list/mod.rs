//! Draw lists: what an actor draws, in the order it lists it, kept until it sends another (ADR-0031). A list holds its
//! sender's own placements and dispatches and calls of its direct children, and the renderer walks the lists from the
//! program's root list, entering each child where it is called, so the order of every draw and dispatch is declared,
//! and one no list walked names is not issued (ADR-0055). Each list is checked whole as it is handled, and what it
//! found is kept, so drawing it looks nothing up and refuses nothing.
//!
//! What a list draws is also what the pointer reaches: each draw says whether it blocks the pointer, lets it through, or
//! is not there to it, and a hit is a place in the sequence that draws nothing but is reached as a draw is. The window
//! delivers each pointer event, top-down, to the sender of each entry drawn under it, until one blocks (ADR-0055).
//!
//! The keys follow the same sequence: an entry declares the keys its sender hears in a window, and the renderer sends
//! each window the declarations its main frame's walk holds, the last called first, whenever they change. The window
//! delivers each key, top-down, to the sender of each declaration whose keys hold it, until one blocks (ADR-0055).

#[cfg(test)]
mod tests;

use crate::dispatch::DispatchId;
use crate::frame::Frame;
use crate::pass::{Pass, PassName};
use crate::placement::PlacementId;
use aethereal_actor::records::Naming;
use aethereal_actor::{Child, ChildOf, ProtocolRef};
use aethereal_data::{ActorId, ActorPath, PathSegment, Plain};
use aethereal_math::{Rect2, Vec2};
use aethereal_window::{Hearer, KeyHearer, KeySet, KeysEntry, Reach};
use std::collections::{HashMap, HashSet};
use std::{slice, vec};

/// What its sender draws, in order: its own placements and dispatches, and the direct children it calls, each drawn
/// where it is called, in place of the sender's last list from the next `Render`. A placement draws in each pass one of
/// its joins names, and a dispatch runs in the compute pass it names, so one list orders the sender's drawing in every
/// pass of every frame (ADR-0055). It has no answer: a list refused is logged, and the last stands.
///
/// A sender with an entry the pointer reaches handles [`Pointed`](aethereal_window::Pointed): it is sent each pointer
/// event that reaches the entry, numbered as it is from 0 in the list. A sender with an entry that declares keys handles
/// [`Keyed`](aethereal_window::Keyed), which each key that reaches the entry is sent as. A list from one that does not
/// handle what its entries would send it is refused.
#[derive(Debug, Clone, PartialEq, Plain)]
#[kind(name = "aether.render.draw_list")]
pub struct DrawList {
    pub entries: Vec<Entry>,
}

/// One entry of a draw list.
#[derive(Debug, Clone, PartialEq, Plain)]
#[kind(name = "aether.render.entry")]
pub enum Entry {
    /// A placement of the sender's, of a mesh or a text, drawn in each pass it joins, reached by the pointer where it
    /// draws as `pointer` says.
    Draw { placement: PlacementId, pointer: Pointer },
    /// A dispatch of the sender's, run in the compute pass it joins. A dispatch asked to run once has no id, and runs
    /// in its pass before those listed there.
    Run(DispatchId),
    /// What a direct child of the sender draws, drawn here.
    Call(Callee),
    /// A place in the pass `pass` of `frame` that draws nothing, reached by the pointer within `rect`, in the window's
    /// surface pixels, as `pointer` says: a modal's backdrop, an invisible handle, or the world's catch-all. A frame
    /// that ends on a texture has no pointer, and nothing there is reached.
    Hit { frame: Frame, pass: PassName, rect: Rect2, pointer: Pointer },
    /// The keys the sender hears in the window whose main frame is `frame`, at this place in the sequence: each key
    /// `keys` holds reaches it, after the declarations called after this one, and, when it `blocks`, none called
    /// before it. Focus is a declaration: a screen that takes the keys lists every key, blocking, while it wants them.
    /// A frame that ends on a texture has no keys, and nothing there hears any.
    Keys { frame: Frame, keys: KeySet, blocks: bool },
}

/// How the pointer reaches an entry where it draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.pointer")]
pub enum Pointer {
    /// It is reached, and what is beneath it is not.
    Blocks,
    /// It is reached, and so is what is beneath it.
    Passes,
    /// It is not reached: the pointer goes to what is beneath it, as if it were not drawn.
    Ignores,
}

/// A direct child of a list's sender, by its segment beneath the sender: what [`Entry::call`] names.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.callee")]
pub struct Callee {
    child: Naming,
}

impl Entry {
    /// A call of `child`, a child of the list's sender, as [`child`](aethereal_actor::child) names it: a place deeper
    /// than a child does not build. The call holds only the child's segment, and the renderer finds the child one
    /// segment beneath whoever sends the list, so a call names only its sender's own direct child.
    #[must_use]
    pub fn call<C: ChildOf>(child: Child<C>) -> Self {
        Self::Call(Callee { child: Naming::of(&child.into_segment()) })
    }

    /// A place in the pass `P` of `frame` that draws nothing, reached by the pointer within `rect` as `pointer` says:
    /// the pass named by its type, as a join names it.
    #[must_use]
    pub const fn hit<P: Pass>(frame: Frame, rect: Rect2, pointer: Pointer) -> Self {
        Self::Hit { frame, pass: PassName::of::<P>(), rect, pointer }
    }

    /// What the renderer keeps of it, the entry numbered `at` of its list: a placement or a dispatch by its id, a call
    /// by its child's, which `child_id` answers for the child's segment beneath the list's sender, and a hit as it is,
    /// each the pointer reaches with the entry of `hearer`, the list's sender, when its sender handles the pointer's
    /// events; and a declaration of keys as the entry of `key_hearer`, the list's sender, when it handles the keys.
    /// Refused when the pointer reaches it or it declares keys, and its sender handles none.
    pub(crate) fn listed(
        &self,
        at: u32,
        (hearer, key_hearer): (Option<ProtocolRef<Hearer>>, Option<ProtocolRef<KeyHearer>>),
        child_id: impl Fn(&PathSegment) -> ActorId,
    ) -> Result<Listed, EntryRefused> {
        let picks = |pointer| match (pointer, hearer) {
            (Pointer::Ignores, _) => Ok(Picks::Ignores),
            (Pointer::Blocks, Some(actor)) => Ok(Picks::Reaches(Reach { actor, entry: at, blocks: true })),
            (Pointer::Passes, Some(actor)) => Ok(Picks::Reaches(Reach { actor, entry: at, blocks: false })),
            (Pointer::Blocks | Pointer::Passes, None) => Err(EntryRefused::Unheard),
        };
        let listed = match self {
            Self::Draw { placement, pointer } => Listed::Draw { placement: placement.id, picks: picks(*pointer)? },
            Self::Run(dispatch) => Listed::Run(dispatch.id),
            Self::Call(callee) => Listed::Call(child_id(&callee.child.segment())),
            Self::Hit { frame, pass, rect, pointer } => {
                Listed::Hit { frame: *frame, pass: *pass, rect: *rect, picks: picks(*pointer)? }
            }
            Self::Keys { frame, keys, blocks } => {
                let actor = key_hearer.ok_or(EntryRefused::KeysUnheard)?;
                Listed::Keys {
                    frame: *frame,
                    entry: KeysEntry { actor, entry: at, keys: keys.clone(), blocks: *blocks },
                }
            }
        };

        Ok(listed)
    }
}

/// One entry of a list the renderer keeps: a placement of its sender's, by its id, drawn as the pointer reaches it, a
/// dispatch of its sender's, by its id, a child its sender calls, by the child's id, or a hit, hit as the pointer
/// reaches it; or the keys its sender hears in a window's main frame, as the window is sent them.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Listed {
    Draw { placement: u64, picks: Picks },
    Run(u64),
    Call(ActorId),
    Hit { frame: Frame, pass: PassName, rect: Rect2, picks: Picks },
    Keys { frame: Frame, entry: KeysEntry },
}

/// What the pointer reaches where an entry draws: nothing, or the entry of its list's sender, which blocks it or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Picks {
    Ignores,
    Reaches(Reach),
}

/// Whether the pointer at the pixel `pixel` of the window reaches the hit `rect`: whether its middle is within it, as
/// a quad drawn over `rect` covers the pixel.
pub(crate) fn holds(rect: Rect2, [x, y]: [u32; 2]) -> bool {
    // A window is far fewer pixels across than an `f32` counts exactly.
    #[allow(clippy::cast_precision_loss)]
    let middle = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);

    rect.contains_point(middle)
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
    /// No dispatch that runs until released is kept under the id: never made, or released.
    Dispatch,
    /// Another actor made the placement or the dispatch, and only its maker lists it.
    NotOwner,
    /// An earlier entry of the list names the same placement or dispatch, or calls the same child.
    Twice,
    /// The pointer reaches the entry, and the list's sender does not handle the pointer's events.
    Unheard,
    /// The entry declares keys, and the list's sender does not handle the keys.
    KeysUnheard,
}

/// Who made what a list names, as its check looks it up: none when nothing of the sort is kept under the id.
pub(crate) trait Listables {
    /// The maker of the placement `placement`, of a mesh or of a text.
    fn placement_maker(&self, placement: u64) -> Option<ActorId>;
    /// The maker of the dispatch `dispatch` that runs until released.
    fn dispatch_maker(&self, dispatch: u64) -> Option<ActorId>;
}

/// What an entry names that a list names once at most: a placement, a dispatch, or a child called.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Named {
    Placement(u64),
    Dispatch(u64),
    Child(ActorId),
}

/// The lists the renderer keeps: one for each actor, its last that was let through, with the program's root list, the
/// roots a window's main frame is walked from.
#[derive(Debug)]
pub(crate) struct Lists {
    roots: Vec<ActorId>,
    kept: HashMap<ActorId, Vec<Listed>>,
}

impl Lists {
    /// No list kept, with the program's root list `roots`, fixed for the program's life (ADR-0055).
    pub fn new(roots: Vec<ActorId>) -> Self {
        Self { roots, kept: HashMap::new() }
    }

    /// The program's root list: the roots a window's main frame is walked from, in draw order.
    pub fn roots(&self) -> &[ActorId] {
        &self.roots
    }

    /// Checks `list` whole, sent by `by`, against what `kept` keeps, and keeps it in place of `by`'s last list: each
    /// placement and dispatch is one `by` made, kept, and no placement, dispatch, or child is named twice. One that
    /// fails is refused, and `by`'s last list stands.
    pub fn check(&mut self, list: Vec<Listed>, by: ActorId, kept: &impl Listables) -> Result<(), ListRefused> {
        let mut seen = HashSet::new();
        for (entry, at) in list.iter().zip(0..) {
            let refused = |why| ListRefused { at, why };
            entry.made_by(by, kept).map_err(refused)?;
            let Some(named) = entry.named() else {
                continue;
            };
            if !seen.insert(named) {
                return Err(refused(EntryRefused::Twice));
            }
        }
        self.kept.insert(by, list);

        Ok(())
    }

    /// Takes the placement or dispatch `id`, which its maker `by` released, out of `by`'s list, keeping the rest in
    /// order.
    pub fn released(&mut self, id: u64, by: ActorId) {
        if let Some(list) = self.kept.get_mut(&by) {
            list.retain(|entry| !entry.names(id));
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
            Listed::Draw { placement, .. } => Some(*placement),
            Listed::Run(_) | Listed::Call(_) | Listed::Hit { .. } | Listed::Keys { .. } => None,
        })
    }

    /// The draws, dispatches, hits, and keys listed from `roots`, in the order of the walk: each root's list in turn,
    /// each entered where it is called, so a list's calls draw between its own entries as it lists them (ADR-0055). An
    /// actor no list from `roots` calls is not drawn.
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

impl<'a> Iterator for Walked<'a> {
    type Item = &'a Listed;

    fn next(&mut self) -> Option<&'a Listed> {
        loop {
            let Some(list) = self.entered.last_mut() else {
                let root = self.roots.next()?;
                self.enter(root);
                continue;
            };
            match list.next() {
                Some(Listed::Call(child)) => self.enter(*child),
                Some(entry @ (Listed::Draw { .. } | Listed::Run(_) | Listed::Hit { .. } | Listed::Keys { .. })) => {
                    return Some(entry);
                }
                None => {
                    self.entered.pop();
                }
            }
        }
    }
}

impl Listed {
    /// Whether it draws or runs what is kept under `id`: a placement or a dispatch.
    fn names(&self, id: u64) -> bool {
        match *self {
            Self::Draw { placement, .. } => placement == id,
            Self::Run(dispatch) => dispatch == id,
            Self::Call(_) | Self::Hit { .. } | Self::Keys { .. } => false,
        }
    }

    /// What it names that a list names once at most: none for a hit or a declaration of keys.
    const fn named(&self) -> Option<Named> {
        match *self {
            Self::Draw { placement, .. } => Some(Named::Placement(placement)),
            Self::Run(dispatch) => Some(Named::Dispatch(dispatch)),
            Self::Call(child) => Some(Named::Child(child)),
            Self::Hit { .. } | Self::Keys { .. } => None,
        }
    }

    /// Checks that `by` made the placement or dispatch it names, as `kept` finds it kept: refused when none is kept
    /// under its id, or another made it.
    fn made_by(&self, by: ActorId, kept: &impl Listables) -> Result<(), EntryRefused> {
        let (maker, unknown) = match *self {
            Self::Draw { placement, .. } => (kept.placement_maker(placement), EntryRefused::Placement),
            Self::Run(dispatch) => (kept.dispatch_maker(dispatch), EntryRefused::Dispatch),
            Self::Call(_) | Self::Hit { .. } | Self::Keys { .. } => return Ok(()),
        };
        match maker {
            Some(maker) if maker == by => Ok(()),
            Some(_) => Err(EntryRefused::NotOwner),
            None => Err(unknown),
        }
    }
}
