//! A program's root list: the roots its renderer draws, in draw order, fixed for the program's life (ADR-0055).

use crate::root_id;
use aethereal_actor::{Actor, ChildOf, HasRoot};
use aethereal_data::{ActorId, PathSegment};

/// One root a program's root list names: its id.
#[derive(Debug, Clone, Copy)]
pub struct Drawn {
    id: ActorId,
}

impl Drawn {
    /// The singleton root of type `A`.
    fn of<A: ChildOf + Actor<Key = ()>>() -> Self {
        Self { id: root_id(&PathSegment::new(A::NAME, &())) }
    }

    /// Its id.
    pub(super) const fn id(self) -> ActorId {
        self.id
    }
}

/// A root list of the program whose roots are `R` (ADR-0055): `()` for none, or a tuple of root types in draw order,
/// each a singleton root of `R`, found at its place in `I` as `Ordered` finds a root. A root list that names a type
/// that is not a root of the program, or is keyed, does not build.
///
/// ```
/// # use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Top};
/// # use aethereal_data::Plain;
/// # use aethereal_substrate::RootList;
/// #
/// # macro_rules! root {
/// #     ($name:ident, $named:literal) => {
/// #         #[derive(Plain)]
/// #         #[kind(name = $named)]
/// #         struct $name;
/// #
/// #         impl ChildOf for $name {
/// #             type Parent = Top;
/// #         }
/// #
/// #         impl Actor for $name {
/// #             type Config = $name;
/// #             type Key = ();
/// #             type Handles = ();
/// #             type Depends = ();
/// #
/// #             fn begin(root: $name, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
/// #                 Ok(root)
/// #             }
/// #
/// #             fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
/// #                 Ok(())
/// #             }
/// #         }
/// #     };
/// # }
/// #
/// # // What a program asks of its root list `D` as it is built, its roots being `R`.
/// # fn drawn<D: RootList<R, I>, R, I>() {}
/// #
/// # root!(World, "aether.example.world");
/// # root!(Overlay, "aether.example.overlay");
/// // The overlay draws over the world, though the program lists it first.
/// drawn::<(World, Overlay), (World, (Overlay, ())), _>();
/// ```
///
/// A root list that names a root the program does not list does not build:
///
/// ```compile_fail
/// # use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Top};
/// # use aethereal_data::Plain;
/// # use aethereal_substrate::RootList;
/// #
/// # macro_rules! root {
/// #     ($name:ident, $named:literal) => {
/// #         #[derive(Plain)]
/// #         #[kind(name = $named)]
/// #         struct $name;
/// #
/// #         impl ChildOf for $name {
/// #             type Parent = Top;
/// #         }
/// #
/// #         impl Actor for $name {
/// #             type Config = $name;
/// #             type Key = ();
/// #             type Handles = ();
/// #             type Depends = ();
/// #
/// #             fn begin(root: $name, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
/// #                 Ok(root)
/// #             }
/// #
/// #             fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
/// #                 Ok(())
/// #             }
/// #         }
/// #     };
/// # }
/// #
/// # // What a program asks of its root list `D` as it is built, its roots being `R`.
/// # fn drawn<D: RootList<R, I>, R, I>() {}
/// #
/// # root!(World, "aether.example.world");
/// # root!(Overlay, "aether.example.overlay");
/// // The program lists the world alone.
/// drawn::<(World, Overlay), (World, ()), _>();
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a root list of this program's roots",
    label = "a root list names only singleton roots the program lists",
    note = "list each root the root list names with `root`"
)]
pub trait RootList<R, I> {
    /// Each root it names, in draw order.
    fn drawn() -> Vec<Drawn>;
}

impl<R> RootList<R, ()> for () {
    fn drawn() -> Vec<Drawn> {
        Vec::new()
    }
}

macro_rules! root_lists {
    ($( [$($root:ident $at:ident)+] )*) => {$(
        impl<R, $($root, $at),+> RootList<R, ($($at,)+)> for ($($root,)+)
        where
            $($root: ChildOf + Actor<Key = ()>, R: HasRoot<$root, $at>,)+
        {
            fn drawn() -> Vec<Drawn> {
                vec![$(Drawn::of::<$root>()),+]
            }
        }
    )*};
}

root_lists! {
    [A IA]
    [A IA B IB]
    [A IA B IB C IC]
    [A IA B IB C IC D ID]
    [A IA B IB C IC D ID E IE]
    [A IA B IB C IC D ID E IE F IF]
    [A IA B IB C IC D ID E IE F IF G IG]
    [A IA B IB C IC D ID E IE F IF G IG H IH]
}
