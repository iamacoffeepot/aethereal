//! Where an actor may stand: the one place each type declares, beneath its parent's type or at the top (ADR-0022).

use crate::declared::{At, Each, Many, One, Pick, Through, TypeList};
use crate::{Actor, ActorRef, ErasedActorRef, Follow, Here, Seated, Skip};
use aethereal_data::KindId;

/// Where an actor of this type may stand: beneath an actor of its [`Parent`](Self::Parent) type, or at the top of
/// the tree as a root when its parent is [`Top`].
///
/// A type declares it once, as its parent is an associated type, so a type has one place, and a type with one parent
/// above it, up to a root, has one path.
///
/// ```
/// use aethereal_actor::{ChildOf, Top};
/// # use aethereal_actor::{Actor, Beginning, Ending, Fault};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # macro_rules! actor {
/// #     ($name:ident) => {
/// #         #[derive(Plain)]
/// #         struct $name;
/// #
/// #         impl Kind for $name {
/// #             const NAME: KindName = KindName::new(concat!("aether.example.", stringify!($name)));
/// #         }
/// #
/// #         impl Actor for $name {
/// #             type Config = $name;
/// #             type Key = ();
/// #             type Handles = ();
/// #             type Depends = ();
/// #             type Spawns = ();
/// #
/// #             fn begin(config: $name, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
/// #                 Ok(config)
/// #             }
/// #
/// #             fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
/// #                 Ok(())
/// #             }
/// #         }
/// #     };
/// # }
/// # actor!(Host);
/// # actor!(Camera);
///
/// impl ChildOf for Host {
///     type Parent = Top;
/// }
///
/// impl ChildOf for Camera {
///     type Parent = Host;
/// }
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not one actor type that declares where it stands",
    label = "`{Self}` declares no one place",
    note = "a type declares where it stands with `impl ChildOf for {Self} {{ type Parent = .. }}`",
    note = "a type that is `Anywhere` or `Recursive` stands in no one place: reach it from a reference held"
)]
pub trait ChildOf: Actor {
    /// What the type stands beneath: [`Top`], one actor type, [`Recursive`] for the top or its own type, or
    /// [`Anywhere`].
    type Parent: Placement;
}

/// A reference to an actor that a child of type `C` may stand beneath, as `C`'s parent says: what
/// [`Turn::child`](crate::Turn::child) looks beneath (ADR-0022). An [`ActorRef<P>`] when `C`'s parent admits `P`, and
/// an [`ErasedActorRef`] when `C` stands [`Anywhere`].
#[diagnostic::on_unimplemented(
    message = "a `{C}` does not stand beneath `{Self}`",
    note = "a type stands only beneath what its `ChildOf` names as its `Parent`"
)]
pub trait ParentOf<C>: Copy {
    /// The actor referred to, and its seat.
    fn seated(self) -> Seated;
}

impl<P: ChildOf, C: ChildOf> ParentOf<C> for ActorRef<P>
where
    C::Parent: Admits<P, C>,
{
    fn seated(self) -> Seated {
        Self::seated(self)
    }
}

impl<C: ChildOf<Parent = Anywhere>> ParentOf<C> for ErasedActorRef {
    fn seated(self) -> Seated {
        Self::seated(self)
    }
}

/// The top of the tree: what a root stands beneath (ADR-0008).
pub struct Top;

/// Beneath any actor, or at the top: the place of a type whose actors any actor may hold, such as a library's, or the
/// chassis make as a root.
pub struct Anywhere;

/// At the top, or beneath an actor of the type's own: the place of a tree of one type, such as one that computes by
/// spawning its own kind. It names no one path, as its depth is a fact of the run.
pub struct Recursive;

/// A place a type may declare as its parent: [`Top`], an actor type that declares its own, [`Recursive`], or
/// [`Anywhere`].
pub trait Placement {
    /// Who may spawn a child that stands here, as the engine reads it.
    const SPAWNER: Spawner;
    /// Whether the program's build check walks a type that stands here when it finds it in a list of what an actor
    /// spawns: [`Follow`] beneath an actor type or [`Anywhere`], and [`Skip`] at the [`Top`], where nothing spawns
    /// it, or [`Recursive`], where only its own type does (ADR-0059).
    type Walk;
}

impl Placement for Top {
    const SPAWNER: Spawner = Spawner::Nobody;
    type Walk = Skip;
}

impl Placement for Anywhere {
    const SPAWNER: Spawner = Spawner::Anyone;
    type Walk = Follow;
}

impl Placement for Recursive {
    const SPAWNER: Spawner = Spawner::Own;
    type Walk = Skip;
}

impl<T: ChildOf> Placement for T {
    const SPAWNER: Spawner = Spawner::Type(KindId::of::<T>());
    type Walk = Follow;
}

/// Who may spawn an actor of a type, as the engine reads the type's parent: what a guest's host checks a guest's
/// spawn by, as [`Admits`] checks a native one where it is built (ADR-0022).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spawner {
    /// No actor: the type stands at the top, and only the chassis makes it.
    Nobody,
    /// An actor of any type.
    Anyone,
    /// An actor of the type's own.
    Own,
    /// An actor of the type of this id.
    Type(KindId),
}

/// What a turn of an actor of type `Me`, whose type stands beneath this place, is handed as its parent by
/// [`Turn::parent`](crate::Turn::parent): an [`ActorRef<P>`] beneath the one type `P`; an `Option<ActorRef<Me>>`
/// beneath [`Recursive`], none for a root; and an `Option<ErasedActorRef>` beneath [`Anywhere`], none for a root, as
/// which it stands beneath is a fact of the run (ADR-0022). [`Top`] has none, so a root's turn has no parent.
#[diagnostic::on_unimplemented(
    message = "`{Self}` has no parent: an actor of a type that stands at the top is a root",
    label = "a root's turn has no `parent()`"
)]
pub trait Parented<Me> {
    /// The parent, as the turn is handed it.
    type Parent;

    /// The parent the actor stands beneath, from `above`, its seat, none for a root.
    fn parent(above: Option<Seated>) -> Self::Parent;
}

impl<Me, P: ChildOf> Parented<Me> for P {
    type Parent = ActorRef<P>;

    fn parent(above: Option<Seated>) -> ActorRef<P> {
        let Some(above) = above else {
            unreachable!("a type beneath one actor type stands only beneath an actor of it, which spawned it");
        };

        ActorRef::new(above)
    }
}

impl<Me> Parented<Me> for Recursive {
    type Parent = Option<ActorRef<Me>>;

    fn parent(above: Option<Seated>) -> Option<ActorRef<Me>> {
        above.map(ActorRef::new)
    }
}

impl<Me> Parented<Me> for Anywhere {
    type Parent = Option<ErasedActorRef>;

    fn parent(above: Option<Seated>) -> Option<ErasedActorRef> {
        above.map(ErasedActorRef::new)
    }
}

/// This place, the parent a type `C` declares, admits a child of `C` spawned by an actor of type `A`: `A` itself as
/// the one parent, any type for [`Anywhere`], and `C`'s own for [`Recursive`]. [`Top`] admits no spawner. A turn's
/// `spawn` builds only when it does (ADR-0022).
///
/// A type may spawn what stands beneath it, and a tree of one type may spawn its own, as `Host` spawns a `Camera` and
/// `Fib` a `Fib`:
///
/// ```
/// use aethereal_actor::{ChildOf, Recursive, Top};
/// # use aethereal_actor::{Actor, Beginning, Ending, Fault, Spawn};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # // An actor type whose `begin` spawns a `$child`, when one is named.
/// # macro_rules! actor {
/// #     ($name:ident $(, $child:ident)?) => {
/// #         #[derive(Plain)]
/// #         struct $name;
/// #
/// #         impl Kind for $name {
/// #             const NAME: KindName = KindName::new(concat!("aether.example.", stringify!($name)));
/// #         }
/// #
/// #         impl Actor for $name {
/// #             type Config = $name;
/// #             type Key = ();
/// #             type Handles = ();
/// #             type Depends = ();
/// #             type Spawns = ($($child,)?);
/// #
/// #             fn begin(config: $name, _turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
/// #                 $(let _ = _turn.spawn::<$child>(&(), $child);)?
/// #                 Ok(config)
/// #             }
/// #
/// #             fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
/// #                 Ok(())
/// #             }
/// #         }
/// #     };
/// # }
/// # actor!(Host, Camera);
/// # actor!(Camera);
/// # actor!(Fib, Fib);
///
/// impl ChildOf for Host {
///     type Parent = Top;
/// }
///
/// impl ChildOf for Camera {
///     type Parent = Host;
/// }
///
/// impl ChildOf for Fib {
///     type Parent = Recursive;
/// }
/// ```
///
/// A type that spawns what stands beneath another does not build, as `Host` spawning a `Lens`:
///
/// ```compile_fail,E0277
/// use aethereal_actor::{ChildOf, Top};
/// # use aethereal_actor::{Actor, Beginning, Ending, Fault, Spawn};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # // An actor type whose `begin` spawns a `$child`, when one is named.
/// # macro_rules! actor {
/// #     ($name:ident $(, $child:ident)?) => {
/// #         #[derive(Plain)]
/// #         struct $name;
/// #
/// #         impl Kind for $name {
/// #             const NAME: KindName = KindName::new(concat!("aether.example.", stringify!($name)));
/// #         }
/// #
/// #         impl Actor for $name {
/// #             type Config = $name;
/// #             type Key = ();
/// #             type Handles = ();
/// #             type Depends = ();
/// #             type Spawns = ($($child,)?);
/// #
/// #             fn begin(config: $name, _turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
/// #                 $(let _ = _turn.spawn::<$child>(&(), $child);)?
/// #                 Ok(config)
/// #             }
/// #
/// #             fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
/// #                 Ok(())
/// #             }
/// #         }
/// #     };
/// # }
/// # actor!(Host, Lens);
/// # actor!(Camera);
/// # actor!(Lens);
///
/// impl ChildOf for Host {
///     type Parent = Top;
/// }
///
/// impl ChildOf for Camera {
///     type Parent = Host;
/// }
///
/// impl ChildOf for Lens {
///     type Parent = Camera;
/// }
/// ```
///
/// Nor does one that spawns a tree of another type, as `Host` spawning a `Fib`:
///
/// ```compile_fail,E0277
/// use aethereal_actor::{ChildOf, Recursive, Top};
/// # use aethereal_actor::{Actor, Beginning, Ending, Fault, Spawn};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # // An actor type whose `begin` spawns a `$child`, when one is named.
/// # macro_rules! actor {
/// #     ($name:ident $(, $child:ident)?) => {
/// #         #[derive(Plain)]
/// #         struct $name;
/// #
/// #         impl Kind for $name {
/// #             const NAME: KindName = KindName::new(concat!("aether.example.", stringify!($name)));
/// #         }
/// #
/// #         impl Actor for $name {
/// #             type Config = $name;
/// #             type Key = ();
/// #             type Handles = ();
/// #             type Depends = ();
/// #             type Spawns = ($($child,)?);
/// #
/// #             fn begin(config: $name, _turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
/// #                 $(let _ = _turn.spawn::<$child>(&(), $child);)?
/// #                 Ok(config)
/// #             }
/// #
/// #             fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
/// #                 Ok(())
/// #             }
/// #         }
/// #     };
/// # }
/// # actor!(Host, Fib);
/// # actor!(Fib);
///
/// impl ChildOf for Host {
///     type Parent = Top;
/// }
///
/// impl ChildOf for Fib {
///     type Parent = Recursive;
/// }
/// ```
#[diagnostic::on_unimplemented(
    message = "an actor of type `{A}` may not spawn a `{C}`",
    label = "a `{C}` stands beneath `{Self}`",
    note = "a type spawns only what may stand beneath it: what its `ChildOf` names as its `Parent`"
)]
pub trait Admits<A, C> {}

impl<A: ChildOf, C> Admits<A, C> for A {}

impl<A, C> Admits<A, C> for Anywhere {}

impl<C> Admits<C, C> for Recursive {}

/// This list, the `Spawns` of an actor of type `Me`, lists `C`, at `I`, its place, which the compiler infers as
/// [`Lists`](crate::Lists) finds a kind: a turn's `spawn` builds only when it does, beside [`Admits`], so the program's
/// build check, which follows each type's list, walks every type an actor spawns (ADR-0059).
///
/// A type is found in a tuple, nested as deep as it nests, and in an [`Each<L>`] [`Through`] the path into `L` that
/// the generic code that spawns it states.
///
/// A type that spawns what its list leaves out does not build, as `Host` spawning a `Lens` it does not list:
///
/// ```compile_fail,E0277
/// use aethereal_actor::{ChildOf, Spawn, Top};
/// # use aethereal_actor::{Actor, Beginning, Ending, Fault};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # // An actor type whose `begin` spawns a `$child`, when one is named, and which lists `$spawns`.
/// # macro_rules! actor {
/// #     ($name:ident, $spawns:ty $(, $child:ident)?) => {
/// #         #[derive(Plain)]
/// #         struct $name;
/// #
/// #         impl Kind for $name {
/// #             const NAME: KindName = KindName::new(concat!("aether.example.", stringify!($name)));
/// #         }
/// #
/// #         impl Actor for $name {
/// #             type Config = $name;
/// #             type Key = ();
/// #             type Handles = ();
/// #             type Depends = ();
/// #             type Spawns = $spawns;
/// #
/// #             fn begin(config: $name, _turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
/// #                 $(let _ = _turn.spawn::<$child>(&(), $child);)?
/// #                 Ok(config)
/// #             }
/// #
/// #             fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
/// #                 Ok(())
/// #             }
/// #         }
/// #     };
/// # }
/// // Lists the camera, and spawns the lens.
/// actor!(Host, (Camera,), Lens);
/// # actor!(Camera, ());
/// # actor!(Lens, ());
///
/// impl ChildOf for Host {
///     type Parent = Top;
/// }
///
/// impl ChildOf for Camera {
///     type Parent = Host;
/// }
///
/// impl ChildOf for Lens {
///     type Parent = Host;
/// }
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Me}` spawns `{C}`, which its `Spawns` does not list",
    label = "a `{C}` is spawned here",
    note = "an actor's `Spawns` lists the types it spawns, so the program's build check walks them: add `{C}` to \
            `{Me}`'s"
)]
pub trait ListsChild<Me, C, I> {}

impl<Me, C, L: TypeList, I> ListsChild<Me, C, I> for L where L::Form: ListedBy<L, Me, C, I> {}

/// The list `L`, the `Spawns` of an actor of type `Me`, of this form, lists `C`, at `I`: a list of one when it is `C`,
/// and a list of many when one of its entries lists it.
#[diagnostic::on_unimplemented(
    message = "`{Me}` spawns `{C}`, which its `Spawns` does not list",
    label = "a `{C}` is spawned here",
    note = "an actor's `Spawns` lists the types it spawns, so the program's build check walks them: add `{C}` to \
            `{Me}`'s"
)]
pub trait ListedBy<L, Me, C, I> {}

// A list of one type lists that type: the type sought is here.
impl<Me, C> ListedBy<C, Me, C, Here> for One {}

impl<L: ListsEach<Me, C, I>, Me, C, I> ListedBy<L, Me, C, I> for Many {}

/// One entry of this list of many, the `Spawns` of an actor of type `Me`, lists `C`, at `I`, its place.
#[diagnostic::on_unimplemented(
    message = "`{Me}` spawns `{C}`, which its `Spawns` does not list",
    label = "a `{C}` is spawned here",
    note = "an actor's `Spawns` lists the types it spawns, so the program's build check walks them: add `{C}` to \
            `{Me}`'s"
)]
pub trait ListsEach<Me, C, I> {}

impl<Me, C, L, P: Pick<L, Picked = C>> ListsEach<Me, C, Through<P>> for Each<L> {}

macro_rules! tuples {
    ($( [$($name:ident $place:literal)*] )*) => {$(
        tuples!(@lists [$($name)*] $($name $place)*);
    )*};
    (@lists $all:tt $($name:ident $place:literal)*) => {$(
        tuples!(@list $all $name $place);
    )*};
    (@list [$($all:ident)*] $name:ident $place:literal) => {
        impl<Me, C, I, $($all),*> ListsEach<Me, C, (At<$place>, I)> for ($($all,)*) where $name: ListsChild<Me, C, I> {}
    };
}

tuples! {
    [A 0]
    [A 0 B 1]
    [A 0 B 1 D 2]
    [A 0 B 1 D 2 E 3]
    [A 0 B 1 D 2 E 3 F 4]
    [A 0 B 1 D 2 E 3 F 4 G 5]
    [A 0 B 1 D 2 E 3 F 4 G 5 H 6]
    [A 0 B 1 D 2 E 3 F 4 G 5 H 6 J 7]
    [A 0 B 1 D 2 E 3 F 4 G 5 H 6 J 7 K 8]
    [A 0 B 1 D 2 E 3 F 4 G 5 H 6 J 7 K 8 L 9]
    [A 0 B 1 D 2 E 3 F 4 G 5 H 6 J 7 K 8 L 9 N 10]
    [A 0 B 1 D 2 E 3 F 4 G 5 H 6 J 7 K 8 L 9 N 10 O 11]
    [A 0 B 1 D 2 E 3 F 4 G 5 H 6 J 7 K 8 L 9 N 10 O 11 P 12]
    [A 0 B 1 D 2 E 3 F 4 G 5 H 6 J 7 K 8 L 9 N 10 O 11 P 12 Q 13]
    [A 0 B 1 D 2 E 3 F 4 G 5 H 6 J 7 K 8 L 9 N 10 O 11 P 12 Q 13 R 14]
    [A 0 B 1 D 2 E 3 F 4 G 5 H 6 J 7 K 8 L 9 N 10 O 11 P 12 Q 13 R 14 S 15]
}

/// A place at the top: [`Top`], [`Recursive`], which is the top or beneath its own type, and [`Anywhere`].
pub trait AtTop {}

impl AtTop for Top {}

impl AtTop for Recursive {}

impl AtTop for Anywhere {}

/// A type the chassis may make as a root: one whose parent is at the top, [`Top`], [`Recursive`], or [`Anywhere`]. It is never
/// declared; it follows from [`ChildOf`]. A root is a singleton or instanced like any other actor (ADR-0008).
///
/// A type that stands beneath another is not one, and the chassis cannot make it:
///
/// ```compile_fail
/// use aethereal_actor::{ChildOf, Root, Top};
/// # use aethereal_actor::{Actor, Beginning, Ending, Fault};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # macro_rules! actor {
/// #     ($name:ident) => {
/// #         #[derive(Plain)]
/// #         struct $name;
/// #
/// #         impl Kind for $name {
/// #             const NAME: KindName = KindName::new(concat!("aether.example.", stringify!($name)));
/// #         }
/// #
/// #         impl Actor for $name {
/// #             type Config = $name;
/// #             type Key = ();
/// #             type Handles = ();
/// #             type Depends = ();
/// #             type Spawns = ();
/// #
/// #             fn begin(config: $name, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
/// #                 Ok(config)
/// #             }
/// #
/// #             fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
/// #                 Ok(())
/// #             }
/// #         }
/// #     };
/// # }
/// # actor!(Host);
/// # actor!(Camera);
///
/// impl ChildOf for Host {
///     type Parent = Top;
/// }
///
/// impl ChildOf for Camera {
///     type Parent = Host;
/// }
///
/// fn made_by_the_chassis<R: Root>() {}
///
/// made_by_the_chassis::<Camera>();
/// ```
pub trait Root: ChildOf {}

impl<T: ChildOf> Root for T where T::Parent: AtTop {}
