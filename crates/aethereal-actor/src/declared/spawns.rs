//! What an actor spawns: the actor types it stands up beneath itself, which the program's build check walks
//! (ADR-0022, ADR-0059).

use super::list::{Each, Joined, Tuple, TypeList};
use crate::ChildOf;
use aethereal_data::KindId;

/// The actor types an actor spawns, a list of them (ADR-0059): `()` for none, one type, a tuple of up to 16 lists,
/// nested, or [`Each<L>`] for a list type parameter, as the types it lists.
///
/// A turn's `spawn::<C>` builds only when `C` is listed here ([`ListsChild`](crate::ListsChild)), beside the check that
/// `C` may stand beneath the spawner's type ([`PlacesBeneath`](crate::PlacesBeneath)). The program's build check follows the list
/// from each root, so a child's reach is checked however deep it is spawned. The engine reads the same list as the
/// kind of each type, which a guest's host checks each spawn its turn makes against (ADR-0015).
pub trait Spawns: TypeList {
    /// The kind of each type, in order, its nested lists' joined.
    const KINDS: &'static [KindId];
}

impl Spawns for () {
    const KINDS: &'static [KindId] = &[];
}

impl<T: ChildOf> Spawns for T {
    const KINDS: &'static [KindId] = &[KindId::of::<T>()];
}

impl<L: Spawns> Spawns for Each<L> {
    const KINDS: &'static [KindId] = L::KINDS;
}

macro_rules! tuples {
    ($( [$($name:ident)*] )*) => {$(
        impl<$($name: Spawns),*> Spawns for ($($name,)*) {
            const KINDS: &'static [KindId] = Tuple::<($($name,)*)>::KINDS.entries();
        }

        impl<$($name: Spawns),*> Tuple<($($name,)*)> {
            /// The kinds of each list, joined.
            const KINDS: &'static Joined<KindId> = &Joined::of(&[$($name::KINDS),*]);
        }
    )*};
}

tuples! {
    [A]
    [A B]
    [A B C]
    [A B C D]
    [A B C D E]
    [A B C D E F]
    [A B C D E F G]
    [A B C D E F G H]
    [A B C D E F G H I]
    [A B C D E F G H I J]
    [A B C D E F G H I J K]
    [A B C D E F G H I J K N]
    [A B C D E F G H I J K N O]
    [A B C D E F G H I J K N O P]
    [A B C D E F G H I J K N O P Q]
    [A B C D E F G H I J K N O P Q S]
}
