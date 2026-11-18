//! What a program's build checks of the types it walks: from each of its roots, through what each type spawns, that
//! every type it reaches depends only on types beneath roots the program has (ADR-0022, ADR-0059).
//!
//! A program's roots are a list, `()` for none and `(R, S)` for the root `R` listed after those of `S`, as a setup's
//! are ([`Ordered`](crate::Ordered)). The walk starts at each, and follows each type's `Spawns`, so a child's reach is
//! checked however deep it is spawned. An instanced root counts through its one entry, however many keys it is made
//! under. A type that stands [`Recursive`](crate::Recursive) is not followed from a list: only its own type spawns
//! one, so every one stands beneath a root of its type, which the walk starts at.
//!
//! ```
//! # use aethereal_actor::{Fault, Fits, Recursive, Top, actor};
//! # use aethereal_data::Plain;
//! # use core::marker::PhantomData;
//! #
//! # macro_rules! actor {
//! #     ($name:ident, $named:literal, $parent:ty, $depends:ty, $spawns:ty) => {
//! #         #[derive(Plain)]
//! #         #[kind(name = $named)]
//! #         struct $name;
//! #
//! #         #[actor(parent = $parent, depends($depends), spawns($spawns))]
//! #         impl $name {
//! #             fn begin(actor: $name) -> Result<Self, Fault> {
//! #                 Ok(actor)
//! #             }
//! #         }
//! #     };
//! # }
//! #
//! # // What a program asks of its roots as it is built.
//! # fn built<R: Fits<I>, I>(_: PhantomData<R>) {}
//! #
//! # actor!(Renderer, "aether.example.renderer", Top, (), ());
//! # actor!(Host, "aether.example.host", Top, (), (Camera,));
//! # actor!(Camera, "aether.example.camera", Host, (), (Lens,));
//! # actor!(Lens, "aether.example.lens", Camera, (Renderer,), ());
//! // A tree of one type spawns its own type: the walk does not follow it back into itself.
//! actor!(Fib, "aether.example.fib", Recursive, (Renderer,), (Fib,));
//!
//! // The lens, two levels beneath the host, depends on the renderer, which the program has.
//! built(PhantomData::<(Host, (Renderer, (Fib, ())))>);
//! ```
//!
//! A program that lacks the root of a type it walks to does not build, however deep that type is spawned:
//!
//! ```compile_fail
//! # use aethereal_actor::{Fault, Fits, Top, actor};
//! # use aethereal_data::Plain;
//! # use core::marker::PhantomData;
//! #
//! # macro_rules! actor {
//! #     ($name:ident, $named:literal, $parent:ty, $depends:ty, $spawns:ty) => {
//! #         #[derive(Plain)]
//! #         #[kind(name = $named)]
//! #         struct $name;
//! #
//! #         #[actor(parent = $parent, depends($depends), spawns($spawns))]
//! #         impl $name {
//! #             fn begin(actor: $name) -> Result<Self, Fault> {
//! #                 Ok(actor)
//! #             }
//! #         }
//! #     };
//! # }
//! #
//! # // What a program asks of its roots as it is built.
//! # fn built<R: Fits<I>, I>(_: PhantomData<R>) {}
//! #
//! # actor!(Renderer, "aether.example.renderer", Top, (), ());
//! # actor!(Host, "aether.example.host", Top, (), (Camera,));
//! # actor!(Camera, "aether.example.camera", Host, (), (Lens,));
//! # actor!(Lens, "aether.example.lens", Camera, (Renderer,), ());
//! // The lens, spawned two levels beneath the host, depends on the renderer, which the program does not have.
//! built(PhantomData::<(Host, ())>);
//! ```
//!
//! Nor does one that lacks the root of a type a list type parameter lists, as a toolbox's tools are:
//!
//! ```compile_fail
//! # use aethereal_actor::{Anywhere, Fault, Fits, Spawns, Top, actor};
//! # use aethereal_data::{Boundary, Decode, DecodeError, Encode, EncodeError, Kind, KindName, Plain, Reader, Schema};
//! # use aethereal_data::{Shape, Writer};
//! # use core::marker::PhantomData;
//! #
//! # macro_rules! actor {
//! #     ($name:ident, $named:literal, $parent:ty, $depends:ty) => {
//! #         #[derive(Plain)]
//! #         #[kind(name = $named)]
//! #         struct $name;
//! #
//! #         #[actor(parent = $parent, depends($depends))]
//! #         impl $name {
//! #             fn begin(actor: $name) -> Result<Self, Fault> {
//! #                 Ok(actor)
//! #             }
//! #         }
//! #     };
//! # }
//! #
//! # // What a program asks of its roots as it is built.
//! # fn built<R: Fits<I>, I>(_: PhantomData<R>) {}
//! #
//! # // A box of the actors `L` lists, which it spawns.
//! # struct Crate<L>(PhantomData<fn() -> L>);
//! #
//! # impl<L> Schema for Crate<L> {
//! #     const SHAPE: Shape = Shape::Struct { name: "Crate", fields: &[] };
//! # }
//! #
//! # impl<L> Kind for Crate<L> {
//! #     const NAME: KindName = KindName::new("aether.example.crate");
//! # }
//! #
//! # impl<L, B: Boundary> Encode<B> for Crate<L> {
//! #     fn encode(&self, _: &mut Writer<'_, B>) -> Result<(), EncodeError> {
//! #         Ok(())
//! #     }
//! # }
//! #
//! # impl<L, B: Boundary> Decode<B> for Crate<L> {
//! #     fn decode(_: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
//! #         Ok(Self(PhantomData))
//! #     }
//! # }
//! #
//! # #[actor(spawns(L))]
//! # impl<L: Spawns + Send + 'static> Crate<L> {
//! #     fn begin(_: Packing) -> Result<Self, Fault> {
//! #         Ok(Self(PhantomData))
//! #     }
//! # }
//! #
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.packing")]
//! # struct Packing;
//! #
//! # actor!(Renderer, "aether.example.renderer", Top, ());
//! # actor!(Brush, "aether.example.brush", Anywhere, ());
//! # actor!(Easel, "aether.example.easel", Anywhere, (Renderer,));
//! // The easel, listed in the crate's list, depends on the renderer, which the program does not have.
//! built(PhantomData::<(Crate<(Brush, (Easel,))>, ())>);
//! ```

use super::{Here, There};
use crate::declared::{Each, Many, One, TypeList};
use crate::{Actor, ChildOf, Placement, Rooted};

/// A program's roots, a list, `()` for none and `(R, S)` for the root `R` listed after those of `S`, fit every type the
/// program walks to from them, at `I`, which the compiler infers: each type each root spawns, however deep, depends
/// only on types beneath the program's roots (ADR-0059).
pub trait Fits<I> {}

impl<S: WalkedFrom<S, I>, I> Fits<I> for S {}

/// Each root of this list, of the program whose roots are `S`, is walked against them, at `I`.
pub trait WalkedFrom<S, I> {}

impl<S> WalkedFrom<S, ()> for () {}

impl<S, R: Walked<S, IR>, Rest: WalkedFrom<S, IS>, IR, IS> WalkedFrom<S, (IR, IS)> for (R, Rest) {}

/// An actor type, walked against the roots `S` of a program, at `I`: each type it depends on stands beneath one of
/// them, and each type it spawns is walked too.
pub trait Walked<S, I> {}

impl<T: Actor, S, ID, IS> Walked<S, (ID, IS)> for T
where
    T::Depends: DependsAmong<T, S, ID>,
    T::Spawns: SpawnsAmong<S, IS>,
{
}

/// Each type of this list, which an actor of type `Me` depends on, stands beneath a root of `S`, at `I`.
pub trait DependsAmong<Me, S, I> {}

impl<Me, S, L: TypeList, I> DependsAmong<Me, S, I> for L where L::Form: DependsBy<L, Me, S, I> {}

/// The list `L`, which an actor of type `Me` depends on, of this form, stands beneath roots of `S`, at `I`.
pub trait DependsBy<L, Me, S, I> {}

impl<L: Rooted, Me, S: HoldsRoot<Me, L, L::Root, I>, I> DependsBy<L, Me, S, I> for One {}

impl<L: DependsEach<Me, S, I>, Me, S, I> DependsBy<L, Me, S, I> for Many {}

/// Each entry of this list of many, which an actor of type `Me` depends on, stands beneath roots of `S`, at `I`.
pub trait DependsEach<Me, S, I> {}

impl<Me, S> DependsEach<Me, S, ()> for () {}

impl<Me, S, L: DependsAmong<Me, S, I>, I> DependsEach<Me, S, I> for Each<L> {}

/// A program's roots hold `R`, the root of `L`, which an actor of type `Me` the program walks to depends on, at `I`,
/// its place, which the compiler infers (ADR-0059).
#[diagnostic::on_unimplemented(
    message = "`{Me}` depends on `{L}`, beneath the root `{R}`, which the program does not have",
    label = "the program's roots, which every type it walks to from them must fit",
    note = "add `{R}` to the program's roots, or drop or split `{Me}`: a dependency is never optional (ADR-0059)"
)]
pub trait HoldsRoot<Me, L, R, I> {}

impl<Me, L, R, S> HoldsRoot<Me, L, R, Here> for (R, S) {}

impl<Me, L, R, H, S: HoldsRoot<Me, L, R, I>, I> HoldsRoot<Me, L, R, There<I>> for (H, S) {}

/// Each type of this list, which an actor spawns, is walked against the roots `S`, at `I`, as where it stands says.
pub trait SpawnsAmong<S, I> {}

impl<S, L: TypeList, I> SpawnsAmong<S, I> for L where L::Form: SpawnsBy<L, S, I> {}

/// The list `L`, which an actor spawns, of this form, is walked against the roots `S`, at `I`.
pub trait SpawnsBy<L, S, I> {}

impl<C: ChildOf, S, I> SpawnsBy<C, S, I> for One where <C::Parent as Placement>::Walk: Steps<C, S, I> {}

impl<L: SpawnsEach<S, I>, S, I> SpawnsBy<L, S, I> for Many {}

/// Each entry of this list of many, which an actor spawns, is walked against the roots `S`, at `I`.
pub trait SpawnsEach<S, I> {}

impl<S> SpawnsEach<S, ()> for () {}

impl<S, L: SpawnsAmong<S, I>, I> SpawnsEach<S, I> for Each<L> {}

/// How the walk steps to a type `C` it reaches in a list of what an actor spawns, against the roots `S`, at `I`: what a
/// [`Placement`]'s `Walk` says.
pub trait Steps<C, S, I> {}

/// The walk follows a type that stands beneath an actor type, or [`Anywhere`](crate::Anywhere): it walks it.
pub enum Follow {}

/// The walk does not follow a type that stands at the [`Top`](crate::Top), which nothing spawns, or one that is
/// [`Recursive`](crate::Recursive), which only its own type spawns: every one stands beneath a root of its type, which
/// the walk starts at, and a type whose list names its own would lead the walk back into itself.
pub enum Skip {}

impl<C: Walked<S, I>, S, I> Steps<C, S, I> for Follow {}

impl<C, S> Steps<C, S, ()> for Skip {}

macro_rules! tuples {
    ($( [$($name:ident $is:ident)*] )*) => {$(
        impl<Me, S, $($name: DependsAmong<Me, S, $is>, $is),*> DependsEach<Me, S, ($($is,)*)> for ($($name,)*) {}

        impl<S, $($name: SpawnsAmong<S, $is>, $is),*> SpawnsEach<S, ($($is,)*)> for ($($name,)*) {}
    )*};
}

tuples! {
    [A IA]
    [A IA B IB]
    [A IA B IB C IC]
    [A IA B IB C IC D ID]
    [A IA B IB C IC D ID E IE]
    [A IA B IB C IC D ID E IE F IF]
    [A IA B IB C IC D ID E IE F IF G IG]
    [A IA B IB C IC D ID E IE F IF G IG H IH]
    [A IA B IB C IC D ID E IE F IF G IG H IH J IJ]
    [A IA B IB C IC D ID E IE F IF G IG H IH J IJ K IK]
    [A IA B IB C IC D ID E IE F IF G IG H IH J IJ K IK L IL]
    [A IA B IB C IC D ID E IE F IF G IG H IH J IJ K IK L IL M IM]
    [A IA B IB C IC D ID E IE F IF G IG H IH J IJ K IK L IL M IM N IN]
    [A IA B IB C IC D ID E IE F IF G IG H IH J IJ K IK L IL M IM N IN O IO]
    [A IA B IB C IC D ID E IE F IF G IG H IH J IJ K IK L IL M IM N IN O IO P IP]
    [A IA B IB C IC D ID E IE F IF G IG H IH J IJ K IK L IL M IM N IN O IO P IP Q IQ]
}
