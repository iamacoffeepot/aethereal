//! What comes back from what an actor sends, which its type must handle: the reply a receiver sends to whoever sent
//! to it, checked against the sender where the sender declares the send (ADR-0016).

use super::{Allows, Carries, Handled, Handles, Protocol, ToAny, ToSender, ToType, Watches};
use crate::Actor;
use aethereal_data::Kind;

/// An actor of type `A` handles whatever comes back from these sends, a tuple of markers: what a type's sends are
/// bounded by, so a type that would be sent a reply it cannot handle does not build (ADR-0016).
///
/// A send to an actor of a type brings back the replies that type's handler
/// of the kind declares, and a send through a named protocol what the
/// protocol answers the kind with. A reply, a watch, and a send through a
/// tuple of kinds bring back nothing here. A fallback does not count: a type
/// that takes a reply lists its kind.
///
/// An asker that asks an actor that replies, and does not handle the answer, does not build:
///
/// ```compile_fail
/// use aethereal_actor::{Actor, Beginning, Ending, Fault, Handles, ToSender, ToType, Turn};
/// use aethereal_data::{Kind, KindName, Plain};
///
/// # #[derive(Plain)]
/// # struct Ask;
/// #
/// # impl Kind for Ask {
/// #     const NAME: KindName = KindName::new("aether.example.ask");
/// # }
/// #
/// # #[derive(Plain)]
/// # struct Answer;
/// #
/// # impl Kind for Answer {
/// #     const NAME: KindName = KindName::new("aether.example.answer");
/// # }
/// #
/// # #[derive(Plain)]
/// # struct Answerer;
/// #
/// # impl Kind for Answerer {
/// #     const NAME: KindName = KindName::new("aether.example.answerer");
/// # }
/// #
/// # impl Actor for Answerer {
/// #     type Config = Answerer;
/// #     type Key = ();
/// #     type Handles = (Ask,);
/// #     type BeginSends = ();
/// #     type EndSends = ();
/// #
/// #     fn begin(answerer: Answerer, _: &mut Beginning<'_, ()>) -> Result<Self, Fault> {
/// #         Ok(answerer)
/// #     }
/// #
/// #     fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// /// Answers each question it is asked.
/// impl Handles<Ask> for Answerer {
///     type Sends = (ToSender<Answer>,);
///
///     fn handle(&mut self, _: Ask, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
///         Ok(turn.reply(&Answer)?)
///     }
/// }
///
/// # #[derive(Plain)]
/// # struct Asker;
/// #
/// # impl Kind for Asker {
/// #     const NAME: KindName = KindName::new("aether.example.asker");
/// # }
/// #
/// /// Asks as it begins, and handles nothing.
/// impl Actor for Asker {
///     type Config = Asker;
///     type Key = ();
///     type Handles = ();
///     type BeginSends = (ToType<Answerer, Ask>,);
///     type EndSends = ();
///
///     fn begin(asker: Asker, _: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
///         Ok(asker)
///     }
///
///     fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
/// ```
///
/// So does one that asks from a handler,
///
/// ```compile_fail
/// # use aethereal_actor::{Actor, Beginning, Carries, Ending, Fault, Handles, Protocol, ToAny, ToSender, ToType, Turn};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # #[derive(Plain)]
/// # struct Ask;
/// #
/// # impl Kind for Ask {
/// #     const NAME: KindName = KindName::new("aether.example.ask");
/// # }
/// #
/// # #[derive(Plain)]
/// # struct Answer;
/// #
/// # impl Kind for Answer {
/// #     const NAME: KindName = KindName::new("aether.example.answer");
/// # }
/// #
/// # #[derive(Plain)]
/// # struct Answerer;
/// #
/// # impl Kind for Answerer {
/// #     const NAME: KindName = KindName::new("aether.example.answerer");
/// # }
/// #
/// # impl Actor for Answerer {
/// #     type Config = Answerer;
/// #     type Key = ();
/// #     type Handles = (Ask,);
/// #     type BeginSends = ();
/// #     type EndSends = ();
/// #
/// #     fn begin(answerer: Answerer, _: &mut Beginning<'_, ()>) -> Result<Self, Fault> {
/// #         Ok(answerer)
/// #     }
/// #
/// #     fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// # impl Handles<Ask> for Answerer {
/// #     type Sends = (ToSender<Answer>,);
/// #
/// #     fn handle(&mut self, _: Ask, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
/// #         Ok(turn.reply(&Answer)?)
/// #     }
/// # }
/// #
/// # struct Asking;
/// #
/// # impl Protocol for Asking {
/// #     type Kinds = (Ask,);
/// # }
/// #
/// # impl Carries<Ask> for Asking {
/// #     type Answers = (Answer,);
/// # }
/// #
/// # #[derive(Plain)]
/// # struct Asker;
/// #
/// # impl Kind for Asker {
/// #     const NAME: KindName = KindName::new("aether.example.asker");
/// # }
/// #
/// impl Actor for Asker {
///     type Config = Asker;
///     type Key = ();
///     type Handles = (Ask,);
///     type BeginSends = ();
///     type EndSends = ();
///
///     fn begin(asker: Asker, _: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
///         Ok(asker)
///     }
///
///     fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
///
/// impl Handles<Ask> for Asker {
///     type Sends = (ToType<Answerer, Ask>,);
///
///     fn handle(&mut self, _: Ask, _: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
/// ```
///
/// one that asks as it ends,
///
/// ```compile_fail
/// # use aethereal_actor::{Actor, Beginning, Carries, Ending, Fault, Handles, Protocol, ToAny, ToSender, ToType, Turn};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # #[derive(Plain)]
/// # struct Ask;
/// #
/// # impl Kind for Ask {
/// #     const NAME: KindName = KindName::new("aether.example.ask");
/// # }
/// #
/// # #[derive(Plain)]
/// # struct Answer;
/// #
/// # impl Kind for Answer {
/// #     const NAME: KindName = KindName::new("aether.example.answer");
/// # }
/// #
/// # #[derive(Plain)]
/// # struct Answerer;
/// #
/// # impl Kind for Answerer {
/// #     const NAME: KindName = KindName::new("aether.example.answerer");
/// # }
/// #
/// # impl Actor for Answerer {
/// #     type Config = Answerer;
/// #     type Key = ();
/// #     type Handles = (Ask,);
/// #     type BeginSends = ();
/// #     type EndSends = ();
/// #
/// #     fn begin(answerer: Answerer, _: &mut Beginning<'_, ()>) -> Result<Self, Fault> {
/// #         Ok(answerer)
/// #     }
/// #
/// #     fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// # impl Handles<Ask> for Answerer {
/// #     type Sends = (ToSender<Answer>,);
/// #
/// #     fn handle(&mut self, _: Ask, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
/// #         Ok(turn.reply(&Answer)?)
/// #     }
/// # }
/// #
/// # struct Asking;
/// #
/// # impl Protocol for Asking {
/// #     type Kinds = (Ask,);
/// # }
/// #
/// # impl Carries<Ask> for Asking {
/// #     type Answers = (Answer,);
/// # }
/// #
/// # #[derive(Plain)]
/// # struct Asker;
/// #
/// # impl Kind for Asker {
/// #     const NAME: KindName = KindName::new("aether.example.asker");
/// # }
/// #
/// impl Actor for Asker {
///     type Config = Asker;
///     type Key = ();
///     type Handles = ();
///     type BeginSends = ();
///     type EndSends = (ToType<Answerer, Ask>,);
///
///     fn begin(asker: Asker, _: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
///         Ok(asker)
///     }
///
///     fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
/// ```
///
/// and one that asks through a named protocol that answers what it does not handle:
///
/// ```compile_fail
/// # use aethereal_actor::{Actor, Beginning, Carries, Ending, Fault, Handles, Protocol, ToAny, ToSender, ToType, Turn};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # #[derive(Plain)]
/// # struct Ask;
/// #
/// # impl Kind for Ask {
/// #     const NAME: KindName = KindName::new("aether.example.ask");
/// # }
/// #
/// # #[derive(Plain)]
/// # struct Answer;
/// #
/// # impl Kind for Answer {
/// #     const NAME: KindName = KindName::new("aether.example.answer");
/// # }
/// #
/// # #[derive(Plain)]
/// # struct Answerer;
/// #
/// # impl Kind for Answerer {
/// #     const NAME: KindName = KindName::new("aether.example.answerer");
/// # }
/// #
/// # impl Actor for Answerer {
/// #     type Config = Answerer;
/// #     type Key = ();
/// #     type Handles = (Ask,);
/// #     type BeginSends = ();
/// #     type EndSends = ();
/// #
/// #     fn begin(answerer: Answerer, _: &mut Beginning<'_, ()>) -> Result<Self, Fault> {
/// #         Ok(answerer)
/// #     }
/// #
/// #     fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// # impl Handles<Ask> for Answerer {
/// #     type Sends = (ToSender<Answer>,);
/// #
/// #     fn handle(&mut self, _: Ask, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
/// #         Ok(turn.reply(&Answer)?)
/// #     }
/// # }
/// #
/// # struct Asking;
/// #
/// # impl Protocol for Asking {
/// #     type Kinds = (Ask,);
/// # }
/// #
/// # impl Carries<Ask> for Asking {
/// #     type Answers = (Answer,);
/// # }
/// #
/// # #[derive(Plain)]
/// # struct Asker;
/// #
/// # impl Kind for Asker {
/// #     const NAME: KindName = KindName::new("aether.example.asker");
/// # }
/// #
/// impl Actor for Asker {
///     type Config = Asker;
///     type Key = ();
///     type Handles = ();
///     type BeginSends = (ToAny<Asking, Ask>,);
///     type EndSends = ();
///
///     fn begin(asker: Asker, _: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
///         Ok(asker)
///     }
///
///     fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
/// ```
pub trait BackTo<A> {}

/// The replies among these sends, a tuple of markers, are each of a kind an actor of type `A` handles: what a send
/// to an actor of a type reads of its receiver's handler (ADR-0016).
pub trait RepliesTo<A> {}

/// An actor of type `A` handles each of these kinds, a tuple of kinds: what a named protocol answers with.
pub trait HandledBy<A> {}

impl<A, T: Actor + Handles<K>, K: Kind + Handled> BackTo<A> for ToType<T, K> where <T as Handles<K>>::Sends: RepliesTo<A>
{}

impl<A, K> BackTo<A> for ToSender<K> {}

impl<A> BackTo<A> for Watches {}

/// A named protocol says what each of its kinds is answered with.
impl<A, P: Protocol + Carries<K>, K: Kind> BackTo<A> for ToAny<P, K> where <P as Carries<K>>::Answers: HandledBy<A> {}

/// A tuple of kinds answers none of them.
impl<A, K> BackTo<A> for ToAny<(), K> {}

impl<A: Handles<K>, K: Kind + Handled> RepliesTo<A> for ToSender<K> {}

impl<A, T, K> RepliesTo<A> for ToType<T, K> {}

impl<A, P, K> RepliesTo<A> for ToAny<P, K> {}

impl<A> RepliesTo<A> for Watches {}

/// This send, of a receiver's handler, replies only with a kind of the tuple `Answers`, found at its place in `I`:
/// what a type that conforms to a named protocol replies with (ADR-0016).
pub trait ReplyWithin<Answers, I> {}

/// The replies among these sends, a tuple of markers, are each of a kind of the tuple `Answers`, found at its place in
/// `Is`.
pub trait RepliesWithin<Answers, Is> {}

impl<Answers: Allows<R, I>, R, I> ReplyWithin<Answers, I> for ToSender<R> {}

impl<Answers, T, K> ReplyWithin<Answers, ()> for ToType<T, K> {}

impl<Answers, P, K> ReplyWithin<Answers, ()> for ToAny<P, K> {}

impl<Answers> ReplyWithin<Answers, ()> for Watches {}

impl<Answers> RepliesWithin<Answers, ()> for () {}

impl<A> BackTo<A> for () {}

impl<A> RepliesTo<A> for () {}

impl<A> HandledBy<A> for () {}

macro_rules! tuples {
    ($( [$($name:ident)*] )*) => {$(
        impl<A, $($name: BackTo<A>),*> BackTo<A> for ($($name,)*) {}

        impl<A, $($name: RepliesTo<A>),*> RepliesTo<A> for ($($name,)*) {}

        impl<A: $(Handles<$name> +)*, $($name: Kind + Handled),*> HandledBy<A> for ($($name,)*) {}

        impl<A, $($name),*, K> BackTo<A> for ToAny<($($name,)*), K> {}
    )*};
}

macro_rules! within {
    ($( [$($name:ident $is:ident)*] )*) => {$(
        impl<Answers, $($name: ReplyWithin<Answers, $is>, $is),*> RepliesWithin<Answers, ($($is,)*)> for ($($name,)*) {}
    )*};
}

within! {
    [B IB]
    [B IB C IC]
    [B IB C IC D ID]
    [B IB C IC D ID E IE]
    [B IB C IC D ID E IE F IF]
    [B IB C IC D ID E IE F IF G IG]
    [B IB C IC D ID E IE F IF G IG H IH]
    [B IB C IC D ID E IE F IF G IG H IH I II]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM]
    [B IB C IC D ID E IE F IF G IG H IH I II J IJ L IL M IM N IN]
}

tuples! {
    [B]
    [B C]
    [B C D]
    [B C D E]
    [B C D E F]
    [B C D E F G]
    [B C D E F G H]
    [B C D E F G H I]
    [B C D E F G H I J]
    [B C D E F G H I J L]
    [B C D E F G H I J L M]
    [B C D E F G H I J L M N]
}
