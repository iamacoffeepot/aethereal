//! Kinds: plain data with a name (ADR-0006).

mod cycle;
pub(crate) mod graph;
mod id;
pub mod layout;
mod name;
#[cfg(test)]
pub mod samples;
mod shape;

pub use cycle::{CycleFault, CycleMember, KindDescription};
pub use graph::{MOST_EDGES, MOST_KINDS, kind_id};
pub use id::KindId;
pub use name::KindName;
pub use shape::{KindPtr, KindShape, Site};

use crate::Schema;
use core::marker::PhantomData;

/// A described type with a name (ADR-0006).
pub trait Kind: Schema {
    /// The kind's name.
    const NAME: KindName;
    /// The kind's node in the graph of kinds its references reach, which its id is computed from (ADR-0006).
    ///
    /// The derive gives a non-generic kind a static node, which may be a member of a cycle. Any other kind's node is
    /// made in a constant, as here, and may not be.
    const KIND: KindPtr = KindPtr::of(Unsited::<Self>::NODE);
}

/// The node of a kind with no static one, made in a constant.
struct Unsited<K: ?Sized>(PhantomData<K>);

impl<K: Kind + ?Sized> Unsited<K> {
    const NODE: &'static KindShape = &KindShape { name: K::NAME, site: Site::Unsited, shape: &K::SHAPE };
}

/// A value as the tuple of kinds it is: a kind is one, `()` none. What a handler of mail returns is read so, as the one
/// kind it answers with or none (ADR-0059).
///
/// It is written here, beside `Kind`, as only the crate that owns `Kind` can say that `()` is none: a crate above could
/// not implement one trait for both `()` and every kind, since this crate could implement `Kind` for `()`.
pub trait IntoKinds {
    /// The kinds, as a tuple: `(K,)` for a kind `K`, `()` for `()`.
    type Kinds;

    /// The value as its kinds.
    fn into_kinds(self) -> Self::Kinds;
}

impl<K: Kind> IntoKinds for K {
    type Kinds = (K,);

    fn into_kinds(self) -> (K,) {
        (self,)
    }
}

impl IntoKinds for () {
    type Kinds = ();

    fn into_kinds(self) {}
}

#[cfg(test)]
mod cycle_tests;
