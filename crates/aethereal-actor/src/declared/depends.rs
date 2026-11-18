//! What an actor reaches by typed path: the actor types it depends on, whose roots each typed path it sends at must
//! stand beneath (ADR-0022, ADR-0059).

use super::At;
use super::list::{Each, Joined, Tuple, TypeList};
use crate::{Actor, Beneath, ChildOf, Here, PathTo, Rooted, TypeSegment};
use core::marker::PhantomData;

/// The actor types an actor reaches by typed path, a list of them (ADR-0059): `()` for none, one type, a tuple of up
/// to 16 lists, nested, or [`Each<L>`] for a list type parameter, as the types it lists.
///
/// A type's root is found up its `ChildOf` chain ([`Rooted`]), and a typed path builds as a target when its root is the
/// root of a type listed here: `(Windows,)` admits `path::<Window>(id)`, as `Window` is a keyed child of the singleton
/// root `Windows`, and `(Lens,)` admits `path::<Camera>(())`, as both stand beneath `Host`. Two types of one root are
/// one entry: a path beneath it could be found at either, and is not inferred.
///
/// The engine reads the same list as the last segment of each type's typed path: a spawn is refused while a chain of
/// singletons in it holds no live actor, and a root is made after each singleton root it names (ADR-0022). The
/// program's build check reads it too, for every type it walks: each type's root must be a root of the program.
pub trait Depends: TypeList {
    /// The last segment of each type's typed path, in order, its nested lists' joined.
    const CHAINS: &'static [&'static TypeSegment];
    /// The root of each type, in order, a tuple for each tuple: what the root of a typed path an actor sends at is
    /// found among.
    type Roots;
}

impl Depends for () {
    const CHAINS: &'static [&'static TypeSegment] = &[];
    type Roots = ();
}

impl<T: ChildOf + PathTo + Rooted> Depends for T {
    const CHAINS: &'static [&'static TypeSegment] = &[Last::<T>::SEGMENT];
    type Roots = T::Root;
}

impl<L: Depends> Depends for Each<L> {
    const CHAINS: &'static [&'static TypeSegment] = L::CHAINS;
    type Roots = L::Roots;
}

/// The last segment of the typed path to `L`, which every path to a type that declares where it stands has.
pub struct Last<L>(PhantomData<L>);

impl<L: ChildOf + PathTo> Last<L> {
    pub const SEGMENT: &'static TypeSegment = match L::BENEATH {
        Beneath::Type(segment) => segment,
        Beneath::Top => panic!("a type that declares where it stands is a segment of its path"),
    };
}

/// An actor of this type depends on a type of the root of `L`, found at `I`, its place in its [`Depends`], which the
/// compiler infers: what a typed path to `L` it sends at, asks at, or finds is checked by (ADR-0059). Generic code that
/// reaches a root on an actor's behalf states it as a bound, as a screen's library does of the renderer.
///
/// A type it depends on admits a typed path to any type beneath the same root: depending on the lens admits the camera
/// above it.
///
/// ```
/// # use aethereal_actor::{Fault, Top, Turn, actor, path};
/// # use aethereal_data::Plain;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.poke")]
/// # struct Poke;
/// #
/// # macro_rules! actor {
/// #     ($name:ident, $named:literal, $parent:ty, $depends:ty, $target:ty) => {
/// #         #[derive(Plain)]
/// #         #[kind(name = $named)]
/// #         struct $name;
/// #
/// #         #[actor(parent = $parent, depends($depends))]
/// #         impl $name {
/// #             fn begin(actor: $name) -> Result<Self, Fault> {
/// #                 Ok(actor)
/// #             }
/// #
/// #             // Pokes the actor at the typed path to `$target` with each poke it is sent.
/// #             #[tell]
/// #             fn poke(&mut self, _: Poke, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
/// #                 turn.to(path::<$target>(())).send(&Poke)?;
/// #
/// #                 Ok(())
/// #             }
/// #         }
/// #     };
/// # }
/// #
/// # actor!(Host, "aether.example.host", Top, (Host,), Host);
/// # actor!(Camera, "aether.example.camera", Host, (Host,), Host);
/// # actor!(Lens, "aether.example.lens", Camera, (Host,), Host);
/// # actor!(Renderer, "aether.example.renderer", Top, (Renderer,), Renderer);
/// actor!(Simulator, "aether.example.simulator", Top, (Renderer, Lens), Camera);
/// ```
///
/// A typed path beneath a root of nothing it depends on does not build:
///
/// ```compile_fail
/// # use aethereal_actor::{Fault, Top, Turn, actor, path};
/// # use aethereal_data::Plain;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.poke")]
/// # struct Poke;
/// #
/// # macro_rules! actor {
/// #     ($name:ident, $named:literal, $parent:ty, $depends:ty, $target:ty) => {
/// #         #[derive(Plain)]
/// #         #[kind(name = $named)]
/// #         struct $name;
/// #
/// #         #[actor(parent = $parent, depends($depends))]
/// #         impl $name {
/// #             fn begin(actor: $name) -> Result<Self, Fault> {
/// #                 Ok(actor)
/// #             }
/// #
/// #             // Pokes the actor at the typed path to `$target` with each poke it is sent.
/// #             #[tell]
/// #             fn poke(&mut self, _: Poke, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
/// #                 turn.to(path::<$target>(())).send(&Poke)?;
/// #
/// #                 Ok(())
/// #             }
/// #         }
/// #     };
/// # }
/// #
/// # actor!(Host, "aether.example.host", Top, (Host,), Host);
/// # actor!(Camera, "aether.example.camera", Host, (Host,), Host);
/// # actor!(Lens, "aether.example.lens", Camera, (Host,), Host);
/// # actor!(Renderer, "aether.example.renderer", Top, (Renderer,), Renderer);
/// // Depends on the renderer alone, and reaches the lens beneath the host.
/// actor!(Simulator, "aether.example.simulator", Top, (Renderer,), Lens);
/// ```
pub trait DependsOn<L, I> {}

impl<Me: Actor, L: Rooted, I> DependsOn<L, I> for Me where <Me::Depends as Depends>::Roots: RootsHold<Me, L, L::Root, I> {}

/// These roots, an actor's [`Depends::Roots`], hold `R`, the root of `L`, found at `I`, its place, which the compiler
/// infers: what a typed path to `L` that an actor of type `Me` sends at is checked by (ADR-0059).
#[diagnostic::on_unimplemented(
    message = "`{Me}` reaches `{L}` beneath `{R}`, the root of nothing it depends on",
    label = "`{L}` is reached here at its typed path",
    note = "an actor's `Depends` names the types it reaches by typed path: add `{L}`, or a type beneath `{R}`, to `{Me}`'s"
)]
pub trait RootsHold<Me, L, R, I> {}

/// This root, of `L`, is `A`, the root of a type an actor of type `Me` depends on: one place of [`RootsHold`].
#[diagnostic::on_unimplemented(
    message = "`{Me}` reaches `{L}` beneath `{Self}`, the root of nothing it depends on",
    label = "`{L}` is reached here at its typed path",
    note = "an actor's `Depends` names the types it reaches by typed path: add `{L}`, or a type beneath `{Self}`, to \
            `{Me}`'s"
)]
pub trait RootIs<Me, L, A> {}

impl<Me, L, R> RootIs<Me, L, R> for R {}

// The roots of one type are its root: the root sought is here.
impl<Me, L, R: RootIs<Me, L, X>, X> RootsHold<Me, L, R, Here> for X {}

macro_rules! tuples {
    ($( [$($name:ident $place:literal)*] )*) => {$(
        impl<$($name: Depends),*> Depends for ($($name,)*) {
            const CHAINS: &'static [&'static TypeSegment] = Tuple::<($($name,)*)>::CHAINS.entries();
            type Roots = ($($name::Roots,)*);
        }

        impl<$($name: Depends),*> Tuple<($($name,)*)> {
            /// The chains of each list, joined.
            const CHAINS: &'static Joined<&'static TypeSegment> = &Joined::of(&[$($name::CHAINS),*]);
        }

        tuples!(@holds [$($name)*] $($name $place)*);
    )*};
    (@holds $all:tt $($name:ident $place:literal)*) => {$(
        tuples!(@hold $all $name $place);
    )*};
    (@hold [$($all:ident)*] $name:ident $place:literal) => {
        impl<Me, Reached, Root, Place, $($all),*> RootsHold<Me, Reached, Root, (At<$place>, Place)> for ($($all,)*)
        where
            $name: RootsHold<Me, Reached, Root, Place>,
        {
        }
    };
}

tuples! {
    [A 0]
    [A 0 B 1]
    [A 0 B 1 C 2]
    [A 0 B 1 C 2 D 3]
    [A 0 B 1 C 2 D 3 E 4]
    [A 0 B 1 C 2 D 3 E 4 F 5]
    [A 0 B 1 C 2 D 3 E 4 F 5 G 6]
    [A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7]
    [A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7 I 8]
    [A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7 I 8 J 9]
    [A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7 I 8 J 9 K 10]
    [A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7 I 8 J 9 K 10 N 11]
    [A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7 I 8 J 9 K 10 N 11 O 12]
    [A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7 I 8 J 9 K 10 N 11 O 12 P 13]
    [A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7 I 8 J 9 K 10 N 11 O 12 P 13 Q 14]
    [A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7 I 8 J 9 K 10 N 11 O 12 P 13 Q 14 S 15]
}
