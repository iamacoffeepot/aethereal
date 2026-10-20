//! Where an actor may stand: the one place each type declares, beneath its parent's type or at the top (ADR-0022).

use crate::Actor;

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
/// #             type BeginSends = ();
/// #             type EndSends = ();
/// #
/// #             fn begin(config: $name, _: &mut Beginning<'_, ()>) -> Result<Self, Fault> {
/// #                 Ok(config)
/// #             }
/// #
/// #             fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
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
    note = "a type beneath a tuple of types or `Anywhere` stands in no one place: reach it from a reference held"
)]
pub trait ChildOf: Actor {
    /// What the type stands beneath: [`Top`], one actor type, a tuple of actor types any of which it may stand
    /// beneath, or [`Anywhere`].
    type Parent: Placement;
}

/// The top of the tree: what a root stands beneath (ADR-0008).
pub struct Top;

/// Beneath any actor: the place of a type whose actors any actor may hold, such as a library's.
pub struct Anywhere;

/// A place a type may declare as its parent: [`Top`], an actor type that declares its own, a tuple of them, or
/// [`Anywhere`].
pub trait Placement {}

impl Placement for Top {}

impl Placement for Anywhere {}

impl<T: ChildOf> Placement for T {}

/// A type the chassis may make as a root: one whose parent is [`Top`]. It is never declared; it follows from
/// [`ChildOf`]. A root is a singleton or instanced like any other actor (ADR-0008).
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
/// #             type BeginSends = ();
/// #             type EndSends = ();
/// #
/// #             fn begin(config: $name, _: &mut Beginning<'_, ()>) -> Result<Self, Fault> {
/// #                 Ok(config)
/// #             }
/// #
/// #             fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
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
pub trait Root: ChildOf<Parent = Top> {}

impl<T: ChildOf<Parent = Top>> Root for T {}

/// Tuples of actor types, each a place any of whose types a type may stand beneath.
macro_rules! tuples {
    ($( [$($name:ident)*] )*) => {$(
        impl<$($name: ChildOf),*> Placement for ($($name,)*) {}
    )*};
}

tuples! {
    [A B]
    [A B C]
    [A B C D]
    [A B C D E]
    [A B C D E F]
    [A B C D E F G]
    [A B C D E F G H]
}
