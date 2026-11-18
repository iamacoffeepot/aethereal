//! What an engine's setup checks of the roots it is made with, as the program is built: that each root depends only on
//! roots made before it (ADR-0022, ADR-0059).
//!
//! A setup's roots are a list, `()` for none and `(R, S)` for the root `R` made after those of `S`. A root
//! [`MadeAfter`] the roots of a list depends on a singleton root only when that root is among them. A type beneath a
//! root, or one with a key, is a place no setup can say is filled, and is checked where its dependent is made.
//!
//! ```
//! # use aethereal_actor::{Actor, Anyone, Beginning, ChildOf, Ending, Fault, Handles, Ordered, Top, Turn};
//! # use aethereal_data::Plain;
//! # use core::marker::PhantomData;
//! #
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.note")]
//! # struct Note;
//! #
//! # macro_rules! root {
//! #     ($name:ident, $named:literal, $parent:ty, $key:ty, $depends:ty) => {
//! #         #[derive(Plain)]
//! #         #[kind(name = $named)]
//! #         struct $name;
//! #
//! #         impl ChildOf for $name {
//! #             type Parent = $parent;
//! #         }
//! #
//! #         impl Actor for $name {
//! #             type Config = $name;
//! #             type Key = $key;
//! #             type Handles = (Note,);
//! #             type Depends = $depends;
//! #             type Spawns = ();
//! #
//! #             fn begin(root: $name, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
//! #                 Ok(root)
//! #             }
//! #
//! #             fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
//! #                 Ok(())
//! #             }
//! #         }
//! #
//! #         impl Handles<Note> for $name {
//! #             type From = Anyone;
//! #
//! #             fn handle(&mut self, _: Note, _: &mut Turn<'_, Self>) -> Result<(), Fault> {
//! #                 Ok(())
//! #             }
//! #         }
//! #     };
//! # }
//! #
//! # // What an engine's setup asks of its roots as it is handed the engine.
//! # fn ready<S: Ordered<I>, I>(_: PhantomData<S>) {}
//! #
//! # root!(Lighthouse, "aether.example.lighthouse", Top, (), ());
//! # root!(Buoy, "aether.example.buoy", Top, u64, ());
//! # root!(Lamp, "aether.example.lamp", Lighthouse, (), ());
//! # root!(Sailor, "aether.example.sailor", Top, (), (Buoy, Lamp));
//! // A root that depends only on a keyed root, and on a type beneath another root, needs nothing made before it: those
//! // places are checked where their dependents are made.
//! ready(PhantomData::<(Sailor, ())>);
//! ```
//!
//! A root that depends on a singleton root needs it made before it, whether it reaches it as it begins, in a handler,
//! or as it ends:
//!
//! ```compile_fail
//! # use aethereal_actor::{Actor, Anyone, Beginning, ChildOf, Ending, Fault, Handles, Ordered, Top, Turn};
//! # use aethereal_data::Plain;
//! # use core::marker::PhantomData;
//! #
//! # #[derive(Plain)]
//! # #[kind(name = "aether.example.note")]
//! # struct Note;
//! #
//! # macro_rules! root {
//! #     ($name:ident, $named:literal, $depends:ty) => {
//! #         #[derive(Plain)]
//! #         #[kind(name = $named)]
//! #         struct $name;
//! #
//! #         impl ChildOf for $name {
//! #             type Parent = Top;
//! #         }
//! #
//! #         impl Actor for $name {
//! #             type Config = $name;
//! #             type Key = ();
//! #             type Handles = (Note,);
//! #             type Depends = $depends;
//! #             type Spawns = ();
//! #
//! #             fn begin(root: $name, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
//! #                 Ok(root)
//! #             }
//! #
//! #             fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
//! #                 Ok(())
//! #             }
//! #         }
//! #
//! #         impl Handles<Note> for $name {
//! #             type From = Anyone;
//! #
//! #             fn handle(&mut self, _: Note, _: &mut Turn<'_, Self>) -> Result<(), Fault> {
//! #                 Ok(())
//! #             }
//! #         }
//! #     };
//! # }
//! #
//! # // What an engine's setup asks of its roots as it is handed the engine.
//! # fn ready<S: Ordered<I>, I>(_: PhantomData<S>) {}
//! #
//! # root!(Lighthouse, "aether.example.lighthouse", ());
//! # root!(Lookout, "aether.example.lookout", (Lighthouse,));
//! // The lookout is made before the lighthouse it depends on.
//! ready(PhantomData::<(Lighthouse, (Lookout, ()))>);
//! ```

use crate::declared::{Each, Many, One, TypeList};
use crate::{Actor, ChildOf, Top};
use aethereal_data::{ActorKey, Instanced, Singleton};
use core::marker::PhantomData;

/// The list of roots made before holds the root `L`, at `I`, its place, which the compiler infers.
#[diagnostic::on_unimplemented(
    message = "`{L}` is not a root made before a root that depends on it",
    label = "a root here depends on `{L}`",
    note = "make `{L}` with `add_root` before the root that depends on it"
)]
pub trait HasRoot<L, I> {}

/// The root sought is the first of the list.
pub struct Here;

/// The root sought is among the list's rest, at `I`.
pub struct There<I>(PhantomData<I>);

impl<L, S> HasRoot<L, Here> for (L, S) {}

impl<L, R, S: HasRoot<L, I>, I> HasRoot<L, There<I>> for (R, S) {}

/// Where the type `L` a root depends on stands, a pair of its parent and its keying, places it among the roots `S` made
/// before that root, at `I`: a singleton root among them, and any other anywhere, as no setup can say it is filled.
#[diagnostic::on_unimplemented(
    message = "`{L}` is not a root made before a root that depends on it",
    label = "an engine's setup starts with a root that depends on `{L}`, which no root made before it is",
    note = "make `{L}` with `add_root` before the root that depends on it"
)]
pub trait Placed<L, S, I> {}

impl<L, S: HasRoot<L, I>, I> Placed<L, S, I> for (Top, Singleton) {}

impl<L, S> Placed<L, S, ()> for (Top, Instanced) {}

impl<L, S, P: Actor, K> Placed<L, S, ()> for (P, K) {}

/// Each type of a root's `Depends`, a list of them, is placed among the roots `S` made before it, each at its place in
/// `I`.
#[diagnostic::on_unimplemented(
    message = "`{Self}` names a root not made before the root that depends on it",
    label = "an engine's setup starts with a root that depends on a root made after it",
    note = "make each root with `add_root` before the roots that depend on it"
)]
pub trait NeedsIn<S, I> {}

impl<S, L: TypeList, I> NeedsIn<S, I> for L where L::Form: NeedsBy<L, S, I> {}

/// The list `L` of what a root depends on, of this form, is placed among the roots `S` made before it, at `I`.
pub trait NeedsBy<L, S, I> {}

impl<L: ChildOf, S, I> NeedsBy<L, S, I> for One where (L::Parent, <L::Key as ActorKey>::Keying): Placed<L, S, I> {}

impl<L: NeedsEach<S, I>, S, I> NeedsBy<L, S, I> for Many {}

/// Each entry of this list of many, of what a root depends on, is placed among the roots `S` made before it, at `I`.
pub trait NeedsEach<S, I> {}

impl<S> NeedsEach<S, ()> for () {}

impl<S, L: NeedsIn<S, I>, I> NeedsEach<S, I> for Each<L> {}

/// The root `A` is made after the roots `S`, which hold each singleton root it depends on, at `I`.
pub trait MadeAfter<S, I> {}

impl<A: Actor, S, I> MadeAfter<S, I> for A where A::Depends: NeedsIn<S, I> {}

/// A setup's list of roots, each made after those that follow it in the list, at `I`.
pub trait Ordered<I> {}

impl Ordered<()> for () {}

impl<R: MadeAfter<S, IR>, S: Ordered<IS>, IR, IS> Ordered<(IR, IS)> for (R, S) {}

macro_rules! tuples {
    ($( [$($name:ident $is:ident)*] )*) => {$(
        impl<S, $($name: NeedsIn<S, $is>, $is),*> NeedsEach<S, ($($is,)*)> for ($($name,)*) {}
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
