//! A guard checked at a send's call: whether the receiver's guard admits the sender's type, read from the type the
//! guard hands on (ADR-0058, ADR-0059).

use super::Guard;
use crate::{ActorRef, At, Conforms, ErasedActorRef, Listed, ProtocolRef};

/// A guard `G` of the actor type `A` admits a sender of type `Me`, found at `I`: what a send to an actor of type `A`
/// builds on, beside that `A` handles the kind (ADR-0059).
///
/// It has one impl, over the guard's [`Sent`](super::Sent) type, which every guard names already, so the check reads the
/// type the guard's `admits` answers and cannot drift from it. What it cannot see, the seat a guard compares or the
/// mail it reads, is checked where the mail is delivered (ADR-0058), as is a send through a [`ProtocolRef`], which
/// does not name its receiver's type.
///
/// [`Anyone`](super::Anyone) hands on an [`ErasedActorRef`], which any sender can be, so a send to its handler builds from
/// an actor of any type:
///
/// ```
/// use aethereal_actor::{ActorRef, Anyone, Fault, Handles, Turn};
/// # use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Top};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # // An actor type that stands beneath `$parent` and lists `$kind`s, each handled where the example writes it.
/// # macro_rules! actor {
/// #     ($name:ident, $parent:ty $(, $kind:ty)*) => {
/// #         #[derive(Plain)]
/// #         struct $name;
/// #
/// #         impl Kind for $name {
/// #             const NAME: KindName = KindName::new(concat!("aether.example.", stringify!($name)));
/// #         }
/// #
/// #         impl ChildOf for $name {
/// #             type Parent = $parent;
/// #         }
/// #
/// #         impl Actor for $name {
/// #             type Config = $name;
/// #             type Key = ();
/// #             type Handles = ($($kind,)*);
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
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.ready")]
/// # struct Ready;
/// #
/// # actor!(Pinger, Top, Ready);
///
/// impl Handles<Ready> for Pinger {
///     type From = Anyone;
///     type Returns = ();
///
///     fn handle(&mut self, _: Ready, _: &mut Turn<'_, Self>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
///
/// fn ready<Me>(turn: &mut Turn<'_, Me>, pinger: ActorRef<Pinger>) -> Result<(), Fault> {
///     turn.to(pinger).send(&Ready)?;
///
///     Ok(())
/// }
/// ```
///
/// An [`ActorRef<T>`] guard admits an actor of type `T`, and no other: a `Producer` may not send `Ready` to a
/// `Pinger` whose handler takes it from a `Counter`.
///
/// ```compile_fail,E0277
/// use aethereal_actor::{ActorRef, Fault, Handles, Turn};
/// # use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Top};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # // An actor type that stands beneath `$parent` and lists `$kind`s, each handled where the example writes it.
/// # macro_rules! actor {
/// #     ($name:ident, $parent:ty $(, $kind:ty)*) => {
/// #         #[derive(Plain)]
/// #         struct $name;
/// #
/// #         impl Kind for $name {
/// #             const NAME: KindName = KindName::new(concat!("aether.example.", stringify!($name)));
/// #         }
/// #
/// #         impl ChildOf for $name {
/// #             type Parent = $parent;
/// #         }
/// #
/// #         impl Actor for $name {
/// #             type Config = $name;
/// #             type Key = ();
/// #             type Handles = ($($kind,)*);
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
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.ready")]
/// # struct Ready;
/// #
/// # actor!(Pinger, Top, Ready);
/// # actor!(Counter, Top);
/// # actor!(Producer, Top);
///
/// impl Handles<Ready> for Pinger {
///     type From = ActorRef<Counter>;
///     type Returns = ();
///
///     fn handle(&mut self, _: Ready, _: &mut Turn<'_, Self, ActorRef<Counter>>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
///
/// fn ready(turn: &mut Turn<'_, Producer>, pinger: ActorRef<Pinger>) -> Result<(), Fault> {
///     turn.to(pinger).send(&Ready)?;
///
///     Ok(())
/// }
/// ```
///
/// [`Child<T>`](super::Child) hands on an `ActorRef<T>` too, so it admits only a `T`: whether it is the receiver's own
/// child is a seat, compared at delivery.
///
/// ```compile_fail,E0277
/// use aethereal_actor::guard::Child;
/// use aethereal_actor::{ActorRef, Fault, Handles, Turn};
/// # use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Top};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # // An actor type that stands beneath `$parent` and lists `$kind`s, each handled where the example writes it.
/// # macro_rules! actor {
/// #     ($name:ident, $parent:ty $(, $kind:ty)*) => {
/// #         #[derive(Plain)]
/// #         struct $name;
/// #
/// #         impl Kind for $name {
/// #             const NAME: KindName = KindName::new(concat!("aether.example.", stringify!($name)));
/// #         }
/// #
/// #         impl ChildOf for $name {
/// #             type Parent = $parent;
/// #         }
/// #
/// #         impl Actor for $name {
/// #             type Config = $name;
/// #             type Key = ();
/// #             type Handles = ($($kind,)*);
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
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.ready")]
/// # struct Ready;
/// #
/// # actor!(Pinger, Top, Ready);
/// # actor!(Ponger, Pinger);
/// # actor!(Counter, Top);
///
/// impl Handles<Ready> for Pinger {
///     type From = Child<Ponger>;
///     type Returns = ();
///
///     fn handle(&mut self, _: Ready, _: &mut Turn<'_, Self, Child<Ponger>>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
///
/// fn ready(turn: &mut Turn<'_, Counter>, pinger: ActorRef<Pinger>) -> Result<(), Fault> {
///     turn.to(pinger).send(&Ready)?;
///
///     Ok(())
/// }
/// ```
///
/// [`Parent`](super::Parent) hands on an `ActorRef` of the receiver's parent type, so a `Ponger` that stands beneath a
/// `Pinger` takes `Ready` from no `Counter`.
///
/// ```compile_fail,E0277
/// use aethereal_actor::guard::Parent;
/// use aethereal_actor::{ActorRef, Fault, Handles, Turn};
/// # use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Top};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # // An actor type that stands beneath `$parent` and lists `$kind`s, each handled where the example writes it.
/// # macro_rules! actor {
/// #     ($name:ident, $parent:ty $(, $kind:ty)*) => {
/// #         #[derive(Plain)]
/// #         struct $name;
/// #
/// #         impl Kind for $name {
/// #             const NAME: KindName = KindName::new(concat!("aether.example.", stringify!($name)));
/// #         }
/// #
/// #         impl ChildOf for $name {
/// #             type Parent = $parent;
/// #         }
/// #
/// #         impl Actor for $name {
/// #             type Config = $name;
/// #             type Key = ();
/// #             type Handles = ($($kind,)*);
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
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.ready")]
/// # struct Ready;
/// #
/// # actor!(Pinger, Top);
/// # actor!(Ponger, Pinger, Ready);
/// # actor!(Counter, Top);
///
/// impl Handles<Ready> for Ponger {
///     type From = Parent;
///     type Returns = ();
///
///     fn handle(&mut self, _: Ready, _: &mut Turn<'_, Self, Parent>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
///
/// fn ready(turn: &mut Turn<'_, Counter>, ponger: ActorRef<Ponger>) -> Result<(), Fault> {
///     turn.to(ponger).send(&Ready)?;
///
///     Ok(())
/// }
/// ```
///
/// [`Speaks<P>`](super::Speaks) hands on a [`ProtocolRef<P>`], so it admits an actor whose type handles every kind of
/// `P`: a `Producer` that handles `Pong` but not `Shut` may not subscribe to a `Pinger` that will send it both.
///
/// ```compile_fail,E0277
/// use aethereal_actor::{ActorRef, Anyone, Fault, Handles, Speaks, Turn};
/// # use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Top};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # // An actor type that stands beneath `$parent` and lists `$kind`s, each handled where the example writes it.
/// # macro_rules! actor {
/// #     ($name:ident, $parent:ty $(, $kind:ty)*) => {
/// #         #[derive(Plain)]
/// #         struct $name;
/// #
/// #         impl Kind for $name {
/// #             const NAME: KindName = KindName::new(concat!("aether.example.", stringify!($name)));
/// #         }
/// #
/// #         impl ChildOf for $name {
/// #             type Parent = $parent;
/// #         }
/// #
/// #         impl Actor for $name {
/// #             type Config = $name;
/// #             type Key = ();
/// #             type Handles = ($($kind,)*);
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
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.ready")]
/// # struct Ready;
/// #
/// # actor!(Pinger, Top, Ready);
/// # actor!(Producer, Top, Pong);
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.pong")]
/// struct Pong;
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.shut")]
/// struct Shut;
///
/// impl Handles<Ready> for Pinger {
///     type From = Speaks<(Pong, Shut)>;
///     type Returns = ();
///
///     fn handle(&mut self, _: Ready, _: &mut Turn<'_, Self, Speaks<(Pong, Shut)>>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
///
/// impl Handles<Pong> for Producer {
///     type From = Anyone;
///     type Returns = ();
///
///     fn handle(&mut self, _: Pong, _: &mut Turn<'_, Self>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
///
/// fn subscribe(turn: &mut Turn<'_, Producer>, pinger: ActorRef<Pinger>) -> Result<(), Fault> {
///     turn.to(pinger).send(&Ready)?;
///
///     Ok(())
/// }
/// ```
///
/// A guard of a handler's own is checked through the `Sent` it names: `ItsProcess`, which admits the process a
/// `Spawned` holds, hands on an `ActorRef<Process>`, so only a `Process` may send `Spawned` its `Exited`, and whether it
/// is the one held is read at delivery.
///
/// ```compile_fail,E0277
/// use aethereal_actor::{ActorRef, Checks, Fault, Guard, Handles, Sender, Sent, Turn};
/// # use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Top};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # // An actor type that stands beneath `$parent` and lists `$kind`s, each handled where the example writes it.
/// # macro_rules! actor {
/// #     ($name:ident, $parent:ty $(, $kind:ty)*) => {
/// #         #[derive(Plain)]
/// #         struct $name;
/// #
/// #         impl Kind for $name {
/// #             const NAME: KindName = KindName::new(concat!("aether.example.", stringify!($name)));
/// #         }
/// #
/// #         impl ChildOf for $name {
/// #             type Parent = $parent;
/// #         }
/// #
/// #         impl Actor for $name {
/// #             type Config = $name;
/// #             type Key = ();
/// #             type Handles = ($($kind,)*);
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
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.ready")]
/// # struct Ready;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.example.exited")]
/// # struct Exited;
/// # actor!(Spawned, Top, Exited);
/// # actor!(Process, Top);
/// # actor!(Engines, Top);
///
/// /// The process a spawn runs, while it runs one.
/// struct ItsProcess;
///
/// impl Guard<Spawned> for ItsProcess {
///     type Sent = ActorRef<Process>;
/// }
///
/// impl Checks<Spawned, Exited> for ItsProcess {
///     fn admits(_: &Spawned, _: &Exited, sender: &Sender<'_>) -> Option<ActorRef<Process>> {
///         ActorRef::<Process>::from_sender(sender)
///     }
/// }
///
/// impl Handles<Exited> for Spawned {
///     type From = ItsProcess;
///     type Returns = ();
///
///     fn handle(&mut self, _: Exited, _: &mut Turn<'_, Self, ItsProcess>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
///
/// fn exited(turn: &mut Turn<'_, Engines>, spawned: ActorRef<Spawned>) -> Result<(), Fault> {
///     turn.to(spawned).send(&Exited)?;
///
///     Ok(())
/// }
/// ```
pub trait Admits<A, Me, I> {}

impl<A, Me, I, G: Guard<A>> Admits<A, Me, I> for G where G::Sent: SentBy<Me, I> {}

/// The checked sender a guard hands on can be an actor of type `Me`, found at `I` (ADR-0059).
///
/// `I` is inferred at the call, and is `()` for each of the library's `Sent` types, so generic code that sends to a type
/// of its own parameter states `<T as Handles<K>>::From: Admits<T, Me, ()>` and threads no index. It is there for an
/// enum of senders, whose `SentBy` finds `Me` among its cases as [`Lists`](crate::Lists) finds a kind (ADR-0058).
#[diagnostic::on_unimplemented(
    message = "`{Me}` may not send this mail: the receiver's guard admits only `{Self}`",
    label = "sent here by a `{Me}`",
    note = "a handler's guard admits, at a send's call, the senders its `Sent` type can be: any sender for \
            `ErasedActorRef`, an actor of type `T` for `ActorRef<T>`, and an actor whose type handles every kind of \
            `P` for `ProtocolRef<P>`"
)]
pub trait SentBy<Me, I> {}

/// Any sender.
impl<Me> SentBy<Me, ()> for ErasedActorRef {}

/// An actor of type `T`.
impl<T> SentBy<T, ()> for ActorRef<T> {}

/// An actor whose type handles every kind of `P`, and answers each as a named protocol says: what an `ActorRef<Me>` is
/// made a `ProtocolRef<P>` by, [`Conforms`], so the reference the guard hands on is one `Me` could have made of itself
/// (ADR-0016).
impl<Me: Conforms<P>, P: Listed> SentBy<Me, ()> for ProtocolRef<P> {}

/// One case of an enum of senders, the tuple of its cases' `Sent` types, can be an actor of type `Me`, found at `I`: the
/// case's place, and where that case's own [`SentBy`] finds `Me`. What a derived guard's `SentBy` holds through, so a
/// send to a handler it guards builds from a sender of any of its cases' types (ADR-0058).
///
/// A sender that two cases admit, such as an `ActorRef<Exchange>` case beside an `ErasedActorRef` case, is found at
/// two places, so its send does not build: the index cannot be inferred.
#[diagnostic::on_unimplemented(
    message = "`{Me}` may not send this mail: the receiver's guard admits only the senders of its cases, `{Self}`",
    label = "sent here by a `{Me}`",
    note = "an enum guard admits, at a send's call, a sender that one of its cases' `Sent` types can be"
)]
pub trait CasesSentBy<Me, I> {}

macro_rules! cases {
    ($( [$($name:ident $place:literal)*] )*) => {$(
        cases!(@cases [$($name)*] $($name $place)*);
    )*};
    (@cases $all:tt $($name:ident $place:literal)*) => {$(
        cases!(@case $all $name $place);
    )*};
    (@case [$($all:ident)*] $name:ident $place:literal) => {
        impl<Me, I, $($all),*> CasesSentBy<Me, (At<$place>, I)> for ($($all,)*) where $name: SentBy<Me, I> {}
    };
}

cases! {
    [T0 0]
    [T0 0 T1 1]
    [T0 0 T1 1 T2 2]
    [T0 0 T1 1 T2 2 T3 3]
    [T0 0 T1 1 T2 2 T3 3 T4 4]
    [T0 0 T1 1 T2 2 T3 3 T4 4 T5 5]
    [T0 0 T1 1 T2 2 T3 3 T4 4 T5 5 T6 6]
    [T0 0 T1 1 T2 2 T3 3 T4 4 T5 5 T6 6 T7 7]
    [T0 0 T1 1 T2 2 T3 3 T4 4 T5 5 T6 6 T7 7 T8 8]
    [T0 0 T1 1 T2 2 T3 3 T4 4 T5 5 T6 6 T7 7 T8 8 T9 9]
    [T0 0 T1 1 T2 2 T3 3 T4 4 T5 5 T6 6 T7 7 T8 8 T9 9 T10 10]
    [T0 0 T1 1 T2 2 T3 3 T4 4 T5 5 T6 6 T7 7 T8 8 T9 9 T10 10 T11 11]
    [T0 0 T1 1 T2 2 T3 3 T4 4 T5 5 T6 6 T7 7 T8 8 T9 9 T10 10 T11 11 T12 12]
    [T0 0 T1 1 T2 2 T3 3 T4 4 T5 5 T6 6 T7 7 T8 8 T9 9 T10 10 T11 11 T12 12 T13 13]
    [T0 0 T1 1 T2 2 T3 3 T4 4 T5 5 T6 6 T7 7 T8 8 T9 9 T10 10 T11 11 T12 12 T13 13 T14 14]
    [T0 0 T1 1 T2 2 T3 3 T4 4 T5 5 T6 6 T7 7 T8 8 T9 9 T10 10 T11 11 T12 12 T13 13 T14 14 T15 15]
}
