//! A kind as the layout of a reference reaches it: a node of the graph of kinds that refer to each other through the
//! protocols their references name (ADR-0006, ADR-0016).

use super::graph::kind_id;
use super::{KindId, KindName};
use crate::Shape;
use core::{fmt, ptr};

/// A kind's node: its name, where it is defined, and its shape, whose references reach other kinds' nodes in turn
/// (ADR-0006).
///
/// The kinds that reach each other through references may form a cycle, and the graph of nodes holds it as it is: a
/// non-generic kind's node is a static, which other statics and constants may point at before it is evaluated.
#[derive(Debug)]
pub struct KindShape {
    /// The kind's name.
    pub name: KindName,
    /// Where the kind is defined: what tells two nodes of one kind from two kinds, as a constant cannot compare
    /// pointers.
    pub site: Site,
    /// The kind's shape.
    pub shape: &'static Shape,
}

/// Where a kind is defined.
#[derive(Debug, Clone, Copy)]
pub enum Site {
    /// A non-generic kind whose `Kind` the derive wrote, at the definition `at` names: its module, type, file, line,
    /// and column. Two nodes of one site are one kind. A site is never hashed.
    Defined {
        /// The definition's place, written by the derive.
        at: &'static str,
    },
    /// A generic kind, or one whose `Kind` is written by hand: its node is made in a constant, so the same kind may
    /// be reached through two copies of it. It may not be a member of a cycle.
    Unsited,
}

/// A pointer to a kind's node, as a constant may hold one (ADR-0006).
///
/// A constant that holds a reference to a static has the static evaluated with it, so the node of a kind in a cycle
/// could not name itself through its shape. A raw pointer is not followed until it is read.
///
/// Every `KindPtr` points at a node that lives for the whole program and is never written: `of` takes a `'static`
/// reference, and `defined`'s caller promises an immutable static. That is what makes reading one sound.
#[derive(Clone, Copy)]
pub struct KindPtr(*const KindShape);

// SAFETY: a `KindPtr` is only ever read through, never written through, and what it points at is an immutable
// `KindShape` that lives for the whole program (see the type's doc). `KindShape` is `Sync`, so reading it from several
// threads at once is sound.
unsafe impl Sync for KindPtr {}
// SAFETY: as for `Sync`: the node pointed at outlives every thread and is never written, so a pointer to it may move
// to another thread.
unsafe impl Send for KindPtr {}

impl KindPtr {
    /// A pointer to a node made in a constant: a generic kind's, or one whose `Kind` is written by hand.
    #[must_use]
    pub const fn of(node: &'static KindShape) -> Self {
        Self(node)
    }

    /// A pointer to a non-generic kind's static node, which the derive writes.
    ///
    /// # Safety
    ///
    /// `node` must point at an immutable static `KindShape`: one that lives for the whole program and is never
    /// written. The derive passes `&raw const` of the immutable static it emits beside the kind.
    #[must_use]
    pub const unsafe fn defined(node: *const KindShape) -> Self {
        Self(node)
    }

    /// The node.
    #[must_use]
    pub const fn get(self) -> &'static KindShape {
        // SAFETY: the pointer is non-null, aligned, and points at a `KindShape` that lives for the whole program and is
        // never written, as `of`'s `'static` reference and `defined`'s contract each guarantee, so a shared `'static`
        // reference to it is valid.
        unsafe { &*self.0 }
    }

    /// The id of the kind this is the node of, computed from the graph it reaches (ADR-0006).
    ///
    /// In a constant this is [`KindId::of`](super::KindId::of)'s work, and the derive's constant for each kind with no
    /// parameters, which reports a refused cycle where the kind is defined. When the program runs it walks the graph,
    /// so a running reader asks `KindId::of`, whose id is a constant.
    #[must_use]
    pub const fn id(self) -> KindId {
        kind_id(self)
    }

    /// Whether `other` is this very node, for a reader that runs when the program does: a constant cannot compare
    /// pointers, and tells nodes apart by their sites instead.
    pub(crate) fn is(self, other: Self) -> bool {
        ptr::eq(self.0, other.0)
    }
}

/// Two pointers are equal when they reach one kind: the same id. It walks both kinds' graphs, so it is for a shape
/// compared when the program runs, not for a hot path.
impl PartialEq for KindPtr {
    fn eq(&self, other: &Self) -> bool {
        self.id().same(other.id())
    }
}

impl Eq for KindPtr {}

impl fmt::Debug for KindPtr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("KindPtr").field(&self.get().name).finish()
    }
}
