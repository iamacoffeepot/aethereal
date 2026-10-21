//! What an engine's setup checks of the roots it is made with, as the program is built: that each root reaches at a
//! typed path only roots made before it (ADR-0022).
//!
//! A setup's roots are a list, `()` for none and `(R, S)` for the root `R` made after those of `S`. A root
//! [`MadeAfter`] the roots of a list sends at a typed path whose one segment is a singleton root only when that root
//! is among them. A deeper path, or one with a key, is a place no setup can say is filled, and is checked where its
//! actor is made, as every path is.
//!
//! ```
//! # use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Handles, Ordered, Path, Tells, Top, Turn};
//! # use aethereal_data::{Kind, KindName, Plain};
//! # use core::marker::PhantomData;
//! #
//! # #[derive(Plain)]
//! # struct Note;
//! #
//! # impl Kind for Note {
//! #     const NAME: KindName = KindName::new("aether.example.note");
//! # }
//! #
//! # macro_rules! root {
//! #     ($name:ident, $named:literal, $parent:ty, $key:ty, $begins:ty, $handler:ty, $ends:ty) => {
//! #         #[derive(Plain)]
//! #         struct $name;
//! #
//! #         impl Kind for $name {
//! #             const NAME: KindName = KindName::new($named);
//! #         }
//! #
//! #         impl ChildOf for $name {
//! #             type Parent = $parent;
//! #         }
//! #
//! #         impl Actor for $name {
//! #             type Config = $name;
//! #             type Key = $key;
//! #             type Handles = (Note,);
//! #             type BeginSends = $begins;
//! #             type EndSends = $ends;
//! #
//! #             fn begin(root: $name, _: &mut Beginning<'_, Self, $begins>) -> Result<Self, Fault> {
//! #                 Ok(root)
//! #             }
//! #
//! #             fn end(self, _: &mut Ending<'_, Self, $ends>) -> Result<(), Fault> {
//! #                 Ok(())
//! #             }
//! #         }
//! #
//! #         impl Handles<Note> for $name {
//! #             type Sends = $handler;
//! #
//! #             fn handle(&mut self, _: Note, _: &mut Turn<'_, Self, $handler>) -> Result<(), Fault> {
//! #                 Ok(())
//! #             }
//! #         }
//! #     };
//! # }
//! #
//! # // What an engine's setup asks of its roots as it is handed the engine.
//! # fn ready<S: Ordered<I>, I>(_: PhantomData<S>) {}
//! #
//! # root!(Lighthouse, "aether.example.lighthouse", Top, (), (), (), ());
//! # root!(Buoy, "aether.example.buoy", Top, u64, (), (), ());
//! # root!(Lamp, "aether.example.lamp", Lighthouse, (), (), (), ());
//! # root!(Sailor, "aether.example.sailor", Top, (), (Tells<Path<Buoy>, Note>, Tells<Path<Lamp>, Note>), (), ());
//! // A root that reaches only a keyed path, and one beneath another root, needs nothing made before it: those places
//! // are checked where their actors are made.
//! ready(PhantomData::<(Sailor, ())>);
//! ```
//!
//! A root that reaches a singleton root in a handler, or as it ends, needs it made before it, as one that reaches it
//! as it begins does:
//!
//! ```compile_fail
//! # use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Handles, Ordered, Path, Tells, Top, Turn};
//! # use aethereal_data::{Kind, KindName, Plain};
//! # use core::marker::PhantomData;
//! #
//! # #[derive(Plain)]
//! # struct Note;
//! #
//! # impl Kind for Note {
//! #     const NAME: KindName = KindName::new("aether.example.note");
//! # }
//! #
//! # macro_rules! root {
//! #     ($name:ident, $named:literal, $parent:ty, $key:ty, $begins:ty, $handler:ty, $ends:ty) => {
//! #         #[derive(Plain)]
//! #         struct $name;
//! #
//! #         impl Kind for $name {
//! #             const NAME: KindName = KindName::new($named);
//! #         }
//! #
//! #         impl ChildOf for $name {
//! #             type Parent = $parent;
//! #         }
//! #
//! #         impl Actor for $name {
//! #             type Config = $name;
//! #             type Key = $key;
//! #             type Handles = (Note,);
//! #             type BeginSends = $begins;
//! #             type EndSends = $ends;
//! #
//! #             fn begin(root: $name, _: &mut Beginning<'_, Self, $begins>) -> Result<Self, Fault> {
//! #                 Ok(root)
//! #             }
//! #
//! #             fn end(self, _: &mut Ending<'_, Self, $ends>) -> Result<(), Fault> {
//! #                 Ok(())
//! #             }
//! #         }
//! #
//! #         impl Handles<Note> for $name {
//! #             type Sends = $handler;
//! #
//! #             fn handle(&mut self, _: Note, _: &mut Turn<'_, Self, $handler>) -> Result<(), Fault> {
//! #                 Ok(())
//! #             }
//! #         }
//! #     };
//! # }
//! #
//! # // What an engine's setup asks of its roots as it is handed the engine.
//! # fn ready<S: Ordered<I>, I>(_: PhantomData<S>) {}
//! #
//! # root!(Lighthouse, "aether.example.lighthouse", Top, (), (), (), ());
//! # root!(Buoy, "aether.example.buoy", Top, u64, (), (), ());
//! # root!(Lamp, "aether.example.lamp", Lighthouse, (), (), (), ());
//! # root!(Lookout, "aether.example.lookout", Top, (), (), (Tells<Path<Lighthouse>, Note>,), ());
//! ready(PhantomData::<(Lighthouse, (Lookout, ()))>);
//! ```
//!
//! ```compile_fail
//! # use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Handles, Ordered, Path, Tells, Top, Turn};
//! # use aethereal_data::{Kind, KindName, Plain};
//! # use core::marker::PhantomData;
//! #
//! # #[derive(Plain)]
//! # struct Note;
//! #
//! # impl Kind for Note {
//! #     const NAME: KindName = KindName::new("aether.example.note");
//! # }
//! #
//! # macro_rules! root {
//! #     ($name:ident, $named:literal, $parent:ty, $key:ty, $begins:ty, $handler:ty, $ends:ty) => {
//! #         #[derive(Plain)]
//! #         struct $name;
//! #
//! #         impl Kind for $name {
//! #             const NAME: KindName = KindName::new($named);
//! #         }
//! #
//! #         impl ChildOf for $name {
//! #             type Parent = $parent;
//! #         }
//! #
//! #         impl Actor for $name {
//! #             type Config = $name;
//! #             type Key = $key;
//! #             type Handles = (Note,);
//! #             type BeginSends = $begins;
//! #             type EndSends = $ends;
//! #
//! #             fn begin(root: $name, _: &mut Beginning<'_, Self, $begins>) -> Result<Self, Fault> {
//! #                 Ok(root)
//! #             }
//! #
//! #             fn end(self, _: &mut Ending<'_, Self, $ends>) -> Result<(), Fault> {
//! #                 Ok(())
//! #             }
//! #         }
//! #
//! #         impl Handles<Note> for $name {
//! #             type Sends = $handler;
//! #
//! #             fn handle(&mut self, _: Note, _: &mut Turn<'_, Self, $handler>) -> Result<(), Fault> {
//! #                 Ok(())
//! #             }
//! #         }
//! #     };
//! # }
//! #
//! # // What an engine's setup asks of its roots as it is handed the engine.
//! # fn ready<S: Ordered<I>, I>(_: PhantomData<S>) {}
//! #
//! # root!(Lighthouse, "aether.example.lighthouse", Top, (), (), (), ());
//! # root!(Buoy, "aether.example.buoy", Top, u64, (), (), ());
//! # root!(Lamp, "aether.example.lamp", Lighthouse, (), (), (), ());
//! # root!(Wrecker, "aether.example.wrecker", Top, (), (), (), (Tells<Path<Lighthouse>, Note>,));
//! ready(PhantomData::<(Lighthouse, (Wrecker, ()))>);
//! ```
//!
//! ```compile_fail
//! # use aethereal_actor::{Actor, Answer, Answered, Asks, Beginning, Carries, ChildOf, Ending, Fault, Handles};
//! # use aethereal_actor::{Ordered, Path, Tells, Top, Turn};
//! # use aethereal_data::{Kind, KindName, Plain};
//! # use core::marker::PhantomData;
//! #
//! # #[derive(Plain)]
//! # struct Note;
//! #
//! # impl Kind for Note {
//! #     const NAME: KindName = KindName::new("aether.example.note");
//! # }
//! #
//! # macro_rules! root {
//! #     ($name:ident, $named:literal, $parent:ty, $key:ty, $begins:ty, $handler:ty, $ends:ty) => {
//! #         #[derive(Plain)]
//! #         struct $name;
//! #
//! #         impl Kind for $name {
//! #             const NAME: KindName = KindName::new($named);
//! #         }
//! #
//! #         impl ChildOf for $name {
//! #             type Parent = $parent;
//! #         }
//! #
//! #         impl Actor for $name {
//! #             type Config = $name;
//! #             type Key = $key;
//! #             type Handles = (Note,);
//! #             type BeginSends = $begins;
//! #             type EndSends = $ends;
//! #
//! #             fn begin(root: $name, _: &mut Beginning<'_, Self, $begins>) -> Result<Self, Fault> {
//! #                 Ok(root)
//! #             }
//! #
//! #             fn end(self, _: &mut Ending<'_, Self, $ends>) -> Result<(), Fault> {
//! #                 Ok(())
//! #             }
//! #         }
//! #
//! #         impl Handles<Note> for $name {
//! #             type Sends = $handler;
//! #
//! #             fn handle(&mut self, _: Note, _: &mut Turn<'_, Self, $handler>) -> Result<(), Fault> {
//! #                 Ok(())
//! #             }
//! #         }
//! #     };
//! # }
//! #
//! # // What an engine's setup asks of its roots as it is handed the engine.
//! # fn ready<S: Ordered<I>, I>(_: PhantomData<S>) {}
//! #
//! # root!(Lighthouse, "aether.example.lighthouse", Top, (), (), (), ());
//! # root!(Buoy, "aether.example.buoy", Top, u64, (), (), ());
//! # root!(Lamp, "aether.example.lamp", Lighthouse, (), (), (), ());
//! # impl Carries<Note> for Lighthouse {
//! #     type Answers = (Note,);
//! # }
//! #
//! # #[derive(Plain)]
//! # struct Querent;
//! #
//! # impl Kind for Querent {
//! #     const NAME: KindName = KindName::new("aether.example.querent");
//! # }
//! #
//! # impl ChildOf for Querent {
//! #     type Parent = Top;
//! # }
//! #
//! # impl Actor for Querent {
//! #     type Config = Querent;
//! #     type Key = ();
//! #     type Handles = (Answer<Note, Note>,);
//! #     type BeginSends = (Asks<Path<Lighthouse>, Note, Note>,);
//! #     type EndSends = ();
//! #
//! #     fn begin(querent: Querent, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
//! #         Ok(querent)
//! #     }
//! #
//! #     fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
//! #         Ok(())
//! #     }
//! # }
//! #
//! # impl Handles<Answer<Note, Note>> for Querent {
//! #     type Sends = ();
//! #
//! #     fn handle(&mut self, _: (Answered<Note>, Note), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
//! #         Ok(())
//! #     }
//! # }
//! // An ask at the lighthouse's path needs it as a send there does.
//! ready(PhantomData::<(Lighthouse, (Querent, ()))>);
//! ```

use crate::{Actor, Asks, ChildOf, Handles, Path, Protocol, Spawns, Tells, ToAny, ToSender, ToType};
use crate::{Handled, Top};
use aethereal_data::{ActorKey, Instanced, Singleton};
use core::marker::PhantomData;

/// The list of roots holds the root `L`, at the place `I`, which the compiler infers.
#[diagnostic::on_unimplemented(
    message = "`{L}` is not a root made before a root that reaches it at its typed path",
    label = "a root here reaches `{L}`",
    note = "make `{L}` with `add_root` before the root that reaches it"
)]
pub trait HasRoot<L, I> {}

/// The place of a root in a list of roots: its first.
pub struct Here;

/// The place of a root in a list of roots: past its first, at `I` in the rest.
pub struct There<I>(PhantomData<I>);

impl<L, S> HasRoot<L, Here> for (L, S) {}

impl<L, R, S: HasRoot<L, I>, I> HasRoot<L, There<I>> for (R, S) {}

/// What a send at the typed path of `L` needs of the roots `S` made before: that they hold `L`, at `I`, when its path
/// is `L` alone, a singleton at the top, and nothing otherwise. Implemented for `L`'s parent and keying.
#[diagnostic::on_unimplemented(
    message = "`{L}` is not a root made before a root that reaches it at its typed path",
    label = "an engine's setup starts with a root that reaches `{L}`, which no root made before it is",
    note = "make `{L}` with `add_root` before the root that reaches it"
)]
pub trait Placed<L, S, I> {}

impl<L, S: HasRoot<L, I>, I> Placed<L, S, I> for (Top, Singleton) {}

impl<L, S> Placed<L, S, ()> for (Top, Instanced) {}

impl<L, S, P: Actor, K> Placed<L, S, ()> for (P, K) {}

/// What these sends, a marker or a tuple of them, need of the roots `S` made before, each found at its place in `I`.
#[diagnostic::on_unimplemented(
    message = "`{Self}` reaches at a typed path a root not made before the root that declares it",
    label = "an engine's setup starts with a root that reaches a root made after it",
    note = "make each root with `add_root` before the roots that reach it at its typed path"
)]
pub trait NeedsIn<S, I> {}

impl<S, L: ChildOf, K, I> NeedsIn<S, I> for Tells<Path<L>, K> where
    (L::Parent, <L::Key as ActorKey>::Keying): Placed<L, S, I>
{
}

impl<S, L: ChildOf, K, C, I> NeedsIn<S, I> for Asks<Path<L>, K, C> where
    (L::Parent, <L::Key as ActorKey>::Keying): Placed<L, S, I>
{
}

impl<S, P: Protocol, K, C> NeedsIn<S, ()> for Asks<P, K, C> {}

impl<S, T, K> NeedsIn<S, ()> for ToType<T, K> {}

impl<S, K> NeedsIn<S, ()> for ToSender<K> {}

impl<S, P, K> NeedsIn<S, ()> for ToAny<P, K> {}

impl<S, C> NeedsIn<S, ()> for Spawns<C> {}

impl<S> NeedsIn<S, ()> for () {}

/// What the handlers of an actor of type `A` for these kinds, a tuple of what it handles, need of the roots `S` made
/// before, each found at its place in `I`.
pub trait HandlersNeedIn<A, S, I> {}

impl<A, S> HandlersNeedIn<A, S, ()> for () {}

/// The root of this type needs of the roots `S` made before it only what they hold, as it begins, in each handler, and
/// as it ends, each found at its place in `I`.
pub trait MadeAfter<S, I> {}

impl<A: Actor, S, IB, IH, IE> MadeAfter<S, (IB, IH, IE)> for A
where
    A::BeginSends: NeedsIn<S, IB>,
    A::Handles: HandlersNeedIn<A, S, IH>,
    A::EndSends: NeedsIn<S, IE>,
{
}

/// Each root of this list of roots is [`MadeAfter`] those after it in the list, which were made before it, each
/// found at its place in `I`: what an engine's setup checks as it starts.
pub trait Ordered<I> {}

impl Ordered<()> for () {}

impl<R: MadeAfter<S, IR>, S: Ordered<IS>, IR, IS> Ordered<(IR, IS)> for (R, S) {}

macro_rules! tuples {
    ($( [$($name:ident $is:ident)*] )*) => {$(
        impl<S, $($name: NeedsIn<S, $is>, $is),*> NeedsIn<S, ($($is,)*)> for ($($name,)*) {}

        impl<A, S, $($name: Handled, $is),*> HandlersNeedIn<A, S, ($($is,)*)> for ($($name,)*)
        where
            $(A: Handles<$name>, <A as Handles<$name>>::Sends: NeedsIn<S, $is>,)*
        {
        }
    )*};
}

tuples! {
    [B IB]
    [B IB C IC]
    [B IB C IC D ID]
    [B IB C IC D ID E IE]
    [B IB C IC D ID E IE F IF]
    [B IB C IC D ID E IE F IF G IG]
    [B IB C IC D ID E IE F IF G IG H IH]
    [B IB C IC D ID E IE F IF G IG H IH I II]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL M IM]
}
