//! A kind as the layout of a reference reaches it: a node of the graph of kinds that refer to each other through the
//! protocols their references name (ADR-0006, ADR-0016).

use super::KindName;
use crate::Shape;
use core::fmt;

/// A kind as a reference's layout reaches it: its name, where it is defined, and its shape, whose references reach
/// other kinds' nodes in turn.
///
/// The kinds that reach each other through references form a cycle, and the graph of nodes holds it as it is: a
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
    /// A non-generic kind, at the definition `at` names: its package, version, file, line, column, and type name. Two
    /// nodes of one site are one kind.
    Defined {
        /// The definition's place, written by the derive.
        at: &'static str,
    },
    /// A generic kind, or one whose `Kind` is written by hand: its node is made in a constant, so the same kind may
    /// be reached through two copies. It may not be a member of a cycle.
    Unsited,
}

/// A pointer to a kind's node, as a constant may hold one.
///
/// A constant that holds a reference to a static has the static evaluated with it, so the node of a kind in a cycle
/// could not name itself through its shape. A raw pointer is not followed until it is read.
#[derive(Clone, Copy)]
pub struct KindPtr(*const KindShape);

// SAFETY: a `KindPtr` only ever points at an immutable static or a constant's promoted node, which live for the whole
// program and are never written.
unsafe impl Sync for KindPtr {}
// SAFETY: as for `Sync`.
unsafe impl Send for KindPtr {}

impl KindPtr {
    /// A filler for an array of pointers not yet written; never read.
    pub(crate) const UNSET: Self = Self(core::ptr::null());

    /// A pointer to a node made in a constant: a generic kind's.
    #[must_use]
    pub const fn of(node: &'static KindShape) -> Self {
        Self(node)
    }

    /// A pointer to a non-generic kind's static node.
    ///
    /// # Safety
    ///
    /// `node` must point at an immutable static `KindShape`: the derive writes `&raw const` of the one it emits.
    #[must_use]
    pub const unsafe fn defined(node: *const KindShape) -> Self {
        Self(node)
    }

    /// The node.
    #[must_use]
    pub const fn get(self) -> &'static KindShape {
        // SAFETY: every `KindPtr` points at a node that lives for the whole program, as `of` and `defined` require.
        unsafe { &*self.0 }
    }

    /// Whether `other` is this very node, for a reader that runs when the program does.
    pub(crate) fn is(self, other: Self) -> bool {
        core::ptr::eq(self.0, other.0)
    }
}

/// Two pointers are equal when they reach one kind: the same id.
impl PartialEq for KindPtr {
    fn eq(&self, other: &Self) -> bool {
        super::graph::kind_id(*self).same(super::graph::kind_id(*other))
    }
}

impl Eq for KindPtr {}

impl fmt::Debug for KindPtr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("KindPtr").field(&self.get().name).finish()
    }
}
